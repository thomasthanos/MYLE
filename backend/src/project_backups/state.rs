use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// What runs: one Project Backups job at a time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Job {
    Backup,
    Compare,
}

type Active = Option<(Arc<AtomicBool>, Job)>;

/// One Project Backups job (backups, a comparison) at a time and its cancel
/// switch, and the newest preview (an older one stops when a new one starts).
#[derive(Clone, Default)]
pub struct ProjectBackupsState {
    active: Arc<Mutex<Active>>,
    preview: Arc<AtomicU64>,
}

pub(crate) struct Running {
    state: ProjectBackupsState,
    pub cancel: Arc<AtomicBool>,
}

impl ProjectBackupsState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Active> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn begin(&self, job: Job) -> Result<Running, String> {
        let mut active = self.lock();
        if let Some((_, running)) = active.as_ref() {
            return Err(match running {
                Job::Backup => "A backup is running. Wait for it to finish, or cancel it.",
                Job::Compare => "A comparison is running. Wait for it to finish, or close it.",
            }
            .into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((cancel.clone(), job));
        Ok(Running {
            state: self.clone(),
            cancel,
        })
    }

    pub(crate) fn is_running(&self) -> bool {
        self.lock().is_some()
    }

    /// Cancels the running job if it is `job` (any job: `None`).
    pub(crate) fn cancel(&self, job: Option<Job>) -> bool {
        match self.lock().as_ref() {
            Some((cancel, running)) if job.is_none_or(|job| job == *running) => {
                cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    /// Starts a preview: any older one stops. The closure tells whether this
    /// one should stop.
    pub(crate) fn begin_preview(&self) -> impl Fn() -> bool + Send + 'static {
        let mine = self.preview.fetch_add(1, Ordering::Relaxed) + 1;
        let counter = self.preview.clone();
        move || counter.load(Ordering::Relaxed) != mine
    }

    /// Stops the running preview, if any.
    pub(crate) fn cancel_preview(&self) {
        self.preview.fetch_add(1, Ordering::Relaxed);
    }
}

impl Running {
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        *self.state.lock() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_job_at_a_time_and_a_new_preview_stops_the_old_one() {
        let state = ProjectBackupsState::default();
        let backup = state.begin(Job::Backup).unwrap();
        assert!(state.begin(Job::Compare).err().unwrap().contains("backup"));
        // Closing a comparison never cancels a backup.
        assert!(!state.cancel(Some(Job::Compare)));
        assert!(!backup.cancelled());
        assert!(state.cancel(Some(Job::Backup)));
        assert!(backup.cancelled());
        drop(backup);
        assert!(!state.is_running());

        let first = state.begin_preview();
        assert!(!first());
        let second = state.begin_preview();
        assert!(first() && !second());
        state.cancel_preview();
        assert!(second());
    }
}
