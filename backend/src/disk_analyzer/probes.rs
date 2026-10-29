use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Probe each advertised volume without waiting indefinitely for a missing server.
pub(super) struct ProbeSet<T> {
    active: Mutex<[Option<Arc<Probe<T>>>; 26]>,
}

struct Probe<T> {
    /// None while running; Some(None) when the volume is unavailable.
    value: Mutex<Option<Option<T>>>,
    ready: Condvar,
}

/// A panic or a thread-creation failure also releases the in-flight slot.
struct Completion<T>(Arc<Probe<T>>);

impl<T> Drop for Completion<T> {
    fn drop(&mut self) {
        let mut value = self.0.value.lock().unwrap_or_else(|p| p.into_inner());
        if value.is_none() {
            *value = Some(None);
        }
        self.0.ready.notify_all();
    }
}

impl<T> Default for ProbeSet<T> {
    fn default() -> Self {
        Self { active: Mutex::new(std::array::from_fn(|_| None)) }
    }
}

impl<T: Clone + Send + 'static> ProbeSet<T> {
    pub(super) fn read<F>(&self, mask: u32, timeout: Duration, probe: F) -> Vec<T>
    where
        F: Fn(char) -> Option<T> + Clone + Send + 'static,
    {
        let mut pending = Vec::new();
        {
            let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
            for i in 0..26u8 {
                if mask & (1 << i) == 0 {
                    continue;
                }
                let slot = &mut active[i as usize];
                if let Some(running) = slot
                    && running.value.lock().unwrap_or_else(|p| p.into_inner()).is_none()
                {
                    pending.push(Arc::clone(running));
                    continue;
                }
                // Completed values are refreshed: removable media, free space,
                // and reconnected network volumes can change between reads.
                let running = Arc::new(Probe { value: Mutex::new(None), ready: Condvar::new() });
                *slot = Some(Arc::clone(&running));
                pending.push(Arc::clone(&running));
                let completion = Completion(running);
                let probe = probe.clone();
                // On spawn failure the closure (and Completion) is dropped.
                let _ = std::thread::Builder::new().name(format!("disk-probe-{}", (b'A' + i) as char)).spawn(move || {
                    let value = probe((b'A' + i) as char);
                    *completion.0.value.lock().unwrap_or_else(|p| p.into_inner()) = Some(value);
                });
            }
        }
        let deadline = Instant::now() + timeout;
        let mut values = Vec::new();
        for running in pending {
            let mut value = running.value.lock().unwrap_or_else(|p| p.into_inner());
            while value.is_none() {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                value = running.ready.wait_timeout(value, left).unwrap_or_else(|p| p.into_inner()).0;
            }
            if let Some(Some(value)) = &*value {
                values.push(value.clone());
            }
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex, mpsc};

    #[test]
    fn refreshes_share_a_probe_that_has_not_returned() {
        let probes = ProbeSet::<char>::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let (started, arrivals) = mpsc::channel();
        let probe = {
            let calls = Arc::clone(&calls);
            let released = Arc::clone(&released);
            move |letter| {
                calls.fetch_add(1, Ordering::Relaxed);
                let _ = started.send(());
                let (ready, wake) = &*released;
                let mut ready = ready.lock().unwrap();
                while !*ready {
                    ready = wake.wait(ready).unwrap();
                }
                Some(letter)
            }
        };
        assert!(probes.read(1 << 25, Duration::from_millis(10), probe.clone()).is_empty());
        arrivals.recv_timeout(Duration::from_secs(1)).unwrap();
        for _ in 0..7 {
            assert!(probes.read(1 << 25, Duration::from_millis(10), probe.clone()).is_empty());
        }
        let count = calls.load(Ordering::Relaxed);
        let (ready, wake) = &*released;
        *ready.lock().unwrap() = true;
        wake.notify_all();
        assert_eq!(count, 1, "one stalled volume must not acquire a fresh thread on each refresh");
    }

    #[test]
    fn completed_probes_are_refreshed_including_unavailable_volumes() {
        let probes = ProbeSet::<usize>::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let probe = {
            let calls = Arc::clone(&calls);
            move |_| {
                let number = calls.fetch_add(1, Ordering::Relaxed) + 1;
                if number == 1 { None } else { Some(number) }
            }
        };
        assert!(probes.read(1 << 3, Duration::from_secs(1), probe.clone()).is_empty());
        assert_eq!(probes.read(1 << 3, Duration::from_secs(1), probe.clone()), [2]);
        assert_eq!(probes.read(1 << 3, Duration::from_secs(1), probe), [3]);
    }

    #[test]
    fn a_stalled_volume_does_not_hide_ready_later_volumes() {
        let probes = ProbeSet::<char>::default();
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let probe = {
            let released = Arc::clone(&released);
            move |letter| {
                if letter == 'A' {
                    let (ready, wake) = &*released;
                    let mut ready = ready.lock().unwrap();
                    while !*ready {
                        ready = wake.wait(ready).unwrap();
                    }
                }
                Some(letter)
            }
        };
        let found = probes.read(3, Duration::from_millis(50), probe);
        let (ready, wake) = &*released;
        *ready.lock().unwrap() = true;
        wake.notify_all();
        assert_eq!(found, ['B']);
    }

    #[test]
    fn concurrent_refreshes_share_the_same_in_flight_volume() {
        let probes = ProbeSet::<char>::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let released = Arc::new((Mutex::new(false), Condvar::new()));
        let (started, arrivals) = mpsc::channel();
        let probe = {
            let calls = Arc::clone(&calls);
            let released = Arc::clone(&released);
            move |letter| {
                calls.fetch_add(1, Ordering::Relaxed);
                let _ = started.send(());
                let (ready, wake) = &*released;
                let mut ready = ready.lock().unwrap();
                while !*ready {
                    ready = wake.wait(ready).unwrap();
                }
                Some(letter)
            }
        };
        assert!(probes.read(1, Duration::from_millis(10), probe.clone()).is_empty());
        arrivals.recv_timeout(Duration::from_secs(1)).unwrap();
        std::thread::scope(|scope| {
            for _ in 0..12 {
                let probe = probe.clone();
                let probes = &probes;
                scope.spawn(move || {
                    assert!(probes.read(1, Duration::from_millis(10), probe).is_empty());
                });
            }
        });
        let count = calls.load(Ordering::Relaxed);
        let (ready, wake) = &*released;
        *ready.lock().unwrap() = true;
        wake.notify_all();
        assert_eq!(count, 1);
    }
}
