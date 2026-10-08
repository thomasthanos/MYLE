use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// One Project Backups job (backups, a comparison) at a time, and its cancel
/// switch.
#[derive(Clone, Default)]
pub struct ProjectBackupsState(Arc<Mutex<Option<Arc<AtomicBool>>>>);

pub(crate) struct Running {
    state: ProjectBackupsState,
    pub cancel: Arc<AtomicBool>,
}

impl ProjectBackupsState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Arc<AtomicBool>>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn begin(&self) -> Result<Running, String> {
        let mut active = self.lock();
        if active.is_some() {
            return Err("A backup is already running.".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(cancel.clone());
        Ok(Running {
            state: self.clone(),
            cancel,
        })
    }

    pub(crate) fn is_running(&self) -> bool {
        self.lock().is_some()
    }

    pub(crate) fn cancel(&self) -> bool {
        match self.lock().as_ref() {
            Some(cancel) => {
                cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
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
