//! Registry of running install jobs, so the page can cancel them.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use super::process;
#[derive(Default)]
struct JobState {
    pid: Option<u32>,
    cancelled: bool,
}
#[derive(Default)]
struct Registry {
    jobs: HashMap<String, JobState>,
    /// An exclusive owner is used by operations, such as Spotify restore and
    /// purge, which must never overlap an app installer.
    exclusive_owner: Option<String>,
}
#[derive(Clone, Default)]
pub struct Jobs(Arc<Mutex<Registry>>);
impl Jobs {
    /// Registers a job; it is removed again when the handle is dropped.
    pub fn start(&self, id: &str) -> Result<JobHandle, String> {
        let mut registry = self.lock();
        if registry.exclusive_owner.is_some() {
            return Err("Another maintenance action is currently running.".into());
        }
        if registry.jobs.contains_key(id) {
            return Err("This app already has a running job.".into());
        }
        registry.jobs.insert(id.to_string(), JobState::default());
        Ok(JobHandle {
            jobs: self.clone(),
            id: id.to_string(),
            exclusive: false,
        })
    }
    /// Atomically reserves the whole registry. This closes the check/start
    /// race that would otherwise let an Install Apps job begin during a
    /// destructive maintenance operation.
    pub fn start_exclusive(&self, id: &str) -> Result<JobHandle, String> {
        let mut registry = self.lock();
        if !registry.jobs.is_empty() || registry.exclusive_owner.is_some() {
            return Err("Wait for the current app task to finish first.".into());
        }
        registry.jobs.insert(id.to_string(), JobState::default());
        registry.exclusive_owner = Some(id.to_string());
        Ok(JobHandle {
            jobs: self.clone(),
            id: id.to_string(),
            exclusive: true,
        })
    }
    pub fn cancel(&self, id: &str) {
        let pid = {
            let mut jobs = self.lock();
            let Some(job) = jobs.jobs.get_mut(id) else {
                return;
            };
            job.cancelled = true;
            job.pid
        };
        if let Some(pid) = pid {
            process::kill_tree(pid);
        }
    }
    /// True when nothing is installing or updating, so a bulk winget run on
    /// another page cannot collide with one started here.
    pub fn is_idle(&self) -> bool {
        self.lock().jobs.is_empty()
    }
    pub fn cancel_all(&self) {
        let ids: Vec<String> = self.lock().jobs.keys().cloned().collect();
        for id in ids {
            self.cancel(&id);
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
pub struct JobHandle {
    jobs: Jobs,
    id: String,
    exclusive: bool,
}
impl JobHandle {
    /// Remembers the process to kill on cancel (None once it has exited).
    pub fn set_pid(&self, pid: Option<u32>) {
        if let Some(job) = self.jobs.lock().jobs.get_mut(&self.id) {
            job.pid = pid;
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.jobs
            .lock()
            .jobs
            .get(&self.id)
            .is_some_and(|job| job.cancelled)
    }
}
impl Drop for JobHandle {
    fn drop(&mut self) {
        let mut registry = self.jobs.lock();
        registry.jobs.remove(&self.id);
        if self.exclusive && registry.exclusive_owner.as_deref() == Some(&self.id) {
            registry.exclusive_owner = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_marks_only_that_job_and_handles_are_cleaned_up() {
        let jobs = Jobs::default();
        let a = jobs.start("A").unwrap();
        let b = jobs.start("B").unwrap();
        jobs.cancel("A");
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
        drop(a);
        assert!(!jobs.lock().jobs.contains_key("A"));
        jobs.cancel("unknown"); // no-op
    }
    #[test]
    fn exclusive_job_blocks_both_directions_atomically() {
        let jobs = Jobs::default();
        let ordinary = jobs.start("installer").unwrap();
        assert!(jobs.start_exclusive("purge").is_err());
        drop(ordinary);
        let exclusive = jobs.start_exclusive("purge").unwrap();
        assert!(jobs.start("installer").is_err());
        assert!(jobs.start_exclusive("other").is_err());
        drop(exclusive);
        assert!(jobs.start("installer").is_ok());
    }
}
