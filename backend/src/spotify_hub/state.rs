use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use super::models::{ActiveJob, SpotifyHubAction, SpotifyHubOutcome, SpotifyHubStage};
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
#[derive(Clone)]
struct Running {
    public: ActiveJob,
    cancellation: Cancellation,
}
#[derive(Clone)]
pub struct PreviewGrant {
    pub token: String,
    pub fingerprint: String,
    pub expires_at: u64,
}
#[derive(Default)]
struct Inner {
    active: Option<Running>,
    last_outcome: Option<SpotifyHubOutcome>,
    preview: Option<PreviewGrant>,
}
#[derive(Clone, Default)]
pub struct SpotifyHubState(Arc<Mutex<Inner>>);
impl SpotifyHubState {
    pub fn begin(&self, job_id: String, action: SpotifyHubAction) -> Result<Cancellation, String> {
        let mut inner = self.lock();
        if inner.active.is_some() {
            return Err("A Spotify Hub action is already running.".into());
        }
        let cancellation = Cancellation(Arc::new(AtomicBool::new(false)));
        inner.active = Some(Running {
            public: ActiveJob {
                job_id,
                action,
                stage: SpotifyHubStage::Preparing,
                progress: None,
            },
            cancellation: cancellation.clone(),
        });
        Ok(cancellation)
    }
    pub fn active(&self) -> Option<ActiveJob> {
        self.lock().active.as_ref().map(|job| job.public.clone())
    }
    pub fn last_outcome(&self) -> Option<SpotifyHubOutcome> {
        self.lock().last_outcome.clone()
    }
    pub fn update(&self, job_id: &str, stage: SpotifyHubStage, progress: Option<f64>) {
        if let Some(active) = self
            .lock()
            .active
            .as_mut()
            .filter(|job| job.public.job_id == job_id)
        {
            active.public.stage = stage;
            active.public.progress = progress.map(|value| value.clamp(0.0, 1.0));
        }
    }
    pub fn finish(&self, outcome: SpotifyHubOutcome) {
        let mut inner = self.lock();
        if inner
            .active
            .as_ref()
            .is_some_and(|job| job.public.job_id == outcome.job_id)
        {
            inner.active = None;
        }
        inner.last_outcome = Some(outcome);
    }
    pub fn fail(&self, job_id: &str) {
        let mut inner = self.lock();
        if inner
            .active
            .as_ref()
            .is_some_and(|job| job.public.job_id == job_id)
        {
            inner.active = None;
        }
    }
    pub fn cancel(&self, job_id: &str) {
        if let Some(job) = self
            .lock()
            .active
            .as_ref()
            .filter(|job| job.public.job_id == job_id)
        {
            job.cancellation.cancel();
        }
    }
    pub fn cancel_all(&self) {
        if let Some(job) = self.lock().active.as_ref() {
            job.cancellation.cancel();
        }
    }
    pub fn put_preview(&self, grant: PreviewGrant) {
        self.lock().preview = Some(grant);
    }
    /// Preview tokens are deliberately consumed before the purge starts. A
    /// failed or cancelled purge therefore requires a fresh review.
    pub fn take_preview(&self, token: &str) -> Option<PreviewGrant> {
        let mut inner = self.lock();
        match inner.preview.take() {
            Some(grant) if grant.token == token => Some(grant),
            _ => None,
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_job_can_be_active_and_cancel_is_scoped() {
        let state = SpotifyHubState::default();
        let token = state
            .begin("one".into(), SpotifyHubAction::RestoreSpotify)
            .unwrap();
        assert!(
            state
                .begin("two".into(), SpotifyHubAction::PurgeAll)
                .is_err()
        );
        state.cancel("other");
        assert!(!token.is_cancelled());
        state.cancel("one");
        assert!(token.is_cancelled());
    }
    #[test]
    fn preview_is_one_time() {
        let state = SpotifyHubState::default();
        state.put_preview(PreviewGrant {
            token: "opaque".into(),
            fingerprint: "state".into(),
            expires_at: 1,
        });
        assert!(state.take_preview("opaque").is_some());
        assert!(state.take_preview("opaque").is_none());
    }
}
