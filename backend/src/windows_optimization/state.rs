use std::sync::{Arc, Mutex};
use super::models::AutoLogonOperation;
#[derive(Default)]
struct Inner {
    active_auto_logon: Option<AutoLogonOperation>,
    firmware_restart_active: bool,
}
/// One privileged Windows action at a time: Auto-Logon, or the restart to
/// the firmware.
#[derive(Clone, Default)]
pub struct WindowsOptimizationState(Arc<Mutex<Inner>>);
pub struct FirmwareRestartGuard {
    state: WindowsOptimizationState,
}
/// Auto-Logon is "running" while this lives: however the command ends (an
/// error, or its task dropped halfway), the page never stays stuck on it.
pub struct AutoLogonGuard {
    state: WindowsOptimizationState,
}
impl Drop for AutoLogonGuard {
    fn drop(&mut self) {
        self.state.lock().active_auto_logon = None;
    }
}
impl Drop for FirmwareRestartGuard {
    fn drop(&mut self) {
        self.state.lock().firmware_restart_active = false;
    }
}
impl WindowsOptimizationState {
    fn busy(inner: &Inner) -> bool {
        inner.active_auto_logon.is_some() || inner.firmware_restart_active
    }
    pub fn begin_auto_logon(&self, operation: AutoLogonOperation) -> Result<AutoLogonGuard, String> {
        let mut inner = self.lock();
        if Self::busy(&inner) {
            return Err("Another Windows optimization action is already running.".into());
        }
        inner.active_auto_logon = Some(operation);
        Ok(AutoLogonGuard {
            state: self.clone(),
        })
    }
    pub fn active_auto_logon(&self) -> Option<AutoLogonOperation> {
        self.lock().active_auto_logon
    }
    pub fn begin_firmware_restart(&self) -> Result<FirmwareRestartGuard, String> {
        let mut inner = self.lock();
        if Self::busy(&inner) {
            return Err("Another Windows optimization action is already running.".into());
        }
        inner.firmware_restart_active = true;
        Ok(FirmwareRestartGuard {
            state: self.clone(),
        })
    }
    pub fn firmware_restart_active(&self) -> bool {
        self.lock().firmware_restart_active
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_logon_runs_one_at_a_time() {
        let state = WindowsOptimizationState::default();
        let running = state.begin_auto_logon(AutoLogonOperation::Enable).unwrap();
        assert!(state.begin_auto_logon(AutoLogonOperation::Disable).is_err());
        assert!(state.begin_firmware_restart().is_err());
        drop(running);
        assert!(state.begin_auto_logon(AutoLogonOperation::Disable).is_ok());
    }
    /// A command whose task is dropped while it waits (as a cancelled or
    /// aborted future is) still lets go of Auto-Logon.
    #[tokio::test]
    async fn an_abandoned_auto_logon_lets_go() {
        let state = WindowsOptimizationState::default();
        let task = {
            let state = state.clone();
            tokio::spawn(async move {
                let _running = state.begin_auto_logon(AutoLogonOperation::Enable).unwrap();
                std::future::pending::<()>().await;
            })
        };
        while state.active_auto_logon().is_none() {
            tokio::task::yield_now().await;
        }
        task.abort();
        let _ = task.await;
        assert_eq!(state.active_auto_logon(), None);
    }
    #[test]
    fn firmware_restart_blocks_every_windows_optimization_action_and_clears_on_drop() {
        let state = WindowsOptimizationState::default();
        let restart = state.begin_firmware_restart().unwrap();
        assert!(state.firmware_restart_active());
        assert!(state.begin_auto_logon(AutoLogonOperation::Enable).is_err());
        assert!(state.begin_firmware_restart().is_err());
        drop(restart);
        assert!(!state.firmware_restart_active());
        assert!(state.begin_auto_logon(AutoLogonOperation::Disable).is_ok());
    }
}
