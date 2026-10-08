use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// What runs on a repository: one at a time per repository, so a build and
/// a commit never step on each other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Job {
    Git,
    Build,
    Release,
    Scan,
}

impl Job {
    fn busy_text(self) -> &'static str {
        match self {
            Job::Git => "A git command is running on this project. Wait for it to finish.",
            Job::Build => "A build is running on this project. Wait for it, or cancel it.",
            Job::Release => "A release is running on this project. Wait for it, or cancel it.",
            Job::Scan => "A folder scan is running. Wait for it, or cancel it.",
        }
    }
}

type Active = HashMap<String, (Arc<AtomicBool>, Job)>;

#[derive(Clone, Default)]
pub struct GithubReleasesState {
    active: Arc<Mutex<Active>>,
}

pub(crate) struct Running {
    state: GithubReleasesState,
    key: String,
    pub cancel: Arc<AtomicBool>,
}

impl GithubReleasesState {
    fn lock(&self) -> MutexGuard<'_, Active> {
        self.active.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Starts `job` on the repository `key` (a scan: any key).
    pub(crate) fn begin(&self, key: &str, job: Job) -> Result<Running, String> {
        let mut active = self.lock();
        if let Some((_, running)) = active.get(key) {
            return Err(running.busy_text().into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        active.insert(key.to_string(), (cancel.clone(), job));
        Ok(Running {
            state: self.clone(),
            key: key.to_string(),
            cancel,
        })
    }

    /// What runs on `key`, if anything.
    pub(crate) fn job(&self, key: &str) -> Option<Job> {
        self.lock().get(key).map(|(_, job)| *job)
    }

    pub(crate) fn cancel(&self, key: &str) -> bool {
        match self.lock().get(key) {
            Some((cancel, _)) => {
                cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub(crate) fn cancel_all(&self) {
        for (cancel, _) in self.lock().values() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Running {
    #[cfg(test)]
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.state.lock().remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_job_per_repository() {
        let state = GithubReleasesState::default();
        let build = state.begin("a", Job::Build).unwrap();
        assert!(state.begin("a", Job::Git).err().unwrap().contains("build"));
        let other = state.begin("b", Job::Git).unwrap();
        assert!(state.cancel("a"));
        assert!(build.cancelled() && !other.cancelled());
        drop(build);
        assert_eq!(state.job("a"), None);
        assert_eq!(state.job("b"), Some(Job::Git));
    }
}
