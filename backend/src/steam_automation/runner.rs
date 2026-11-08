//! Keeps exactly the wanted games running, each in its own helper process. The helpers belong to
//! a job object, so Windows ends them if MYLE ends in any way at all.
use std::collections::{BTreeMap, BTreeSet};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use crate::apps::process::JobObject;
const MAX_GAMES: usize = 32;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub(crate) trait Child: Send {
    fn is_running(&mut self) -> bool;
    fn kill(&mut self);
}
pub(crate) trait Spawner: Send + Sync {
    type Child: Child;
    fn spawn(&self, appid: u32) -> Result<Self::Child, String>;
}
pub(crate) trait GameRunner: Send + Sync {
    fn apply(&self, wanted: &BTreeSet<u32>) -> Result<(), String>;
    fn running(&self) -> BTreeSet<u32>;
    fn stop_all(&self);
}
pub(crate) struct ProcessRunner<S: Spawner> {
    spawner: S,
    children: Mutex<BTreeMap<u32, S::Child>>,
}
impl<S: Spawner> ProcessRunner<S> {
    pub(crate) fn new(spawner: S) -> Self {
        Self { spawner, children: Mutex::new(BTreeMap::new()) }
    }
    fn children(&self) -> std::sync::MutexGuard<'_, BTreeMap<u32, S::Child>> {
        let mut children = self.children.lock().unwrap_or_else(|p| p.into_inner());
        children.retain(|_, child| child.is_running());
        children
    }
}
impl<S: Spawner> GameRunner for ProcessRunner<S> {
    fn apply(&self, wanted: &BTreeSet<u32>) -> Result<(), String> {
        let wanted: BTreeSet<u32> = wanted.iter().copied().take(MAX_GAMES).collect();
        let mut children = self.children();
        children.retain(|appid, child| {
            let keep = wanted.contains(appid);
            if !keep {
                child.kill();
            }
            keep
        });
        let mut outcome = Ok(());
        for appid in wanted {
            if children.contains_key(&appid) {
                continue;
            }
            match self.spawner.spawn(appid) {
                Ok(child) => {
                    children.insert(appid, child);
                }
                Err(reason) => outcome = Err(reason),
            }
        }
        outcome
    }
    fn running(&self) -> BTreeSet<u32> {
        self.children().keys().copied().collect()
    }
    fn stop_all(&self) {
        let mut children = self.children();
        for child in children.values_mut() {
            child.kill();
        }
        children.clear();
    }
}
impl Child for std::process::Child {
    fn is_running(&mut self) -> bool {
        matches!(self.try_wait(), Ok(None))
    }
    fn kill(&mut self) {
        let _ = std::process::Child::kill(self);
        let _ = self.wait();
    }
}
/// Starts this same program as a helper for one game.
pub(crate) struct HelperSpawner {
    exe: PathBuf,
    job: JobObject,
}
impl HelperSpawner {
    pub(crate) fn new() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let job = JobObject::new().ok_or("Could not set up the process group for Steam games.")?;
        Ok(Self { exe, job })
    }
}
impl Spawner for HelperSpawner {
    type Child = std::process::Child;
    fn spawn(&self, appid: u32) -> Result<std::process::Child, String> {
        let mut child = Command::new(&self.exe)
            .arg(format!("--steam-run={appid}"))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
        // A helper outside the job could outlive MYLE, so one that cannot join does not run.
        if !self.job.assign(child.as_raw_handle()) {
            Child::kill(&mut child);
            return Err("Could not keep a Steam game under MYLE's control.".to_string());
        }
        Ok(child)
    }
}
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeRunner {
    pub current: Mutex<BTreeSet<u32>>,
    pub stops: Mutex<u32>,
}
#[cfg(test)]
impl GameRunner for FakeRunner {
    fn apply(&self, wanted: &BTreeSet<u32>) -> Result<(), String> {
        *self.current.lock().unwrap() = wanted.clone();
        Ok(())
    }
    fn running(&self) -> BTreeSet<u32> {
        self.current.lock().unwrap().clone()
    }
    fn stop_all(&self) {
        *self.stops.lock().unwrap() += 1;
        self.current.lock().unwrap().clear();
    }
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use super::*;
    #[derive(Default)]
    struct Log {
        spawned: Vec<u32>,
        killed: Vec<u32>,
    }
    struct FakeChild {
        appid: u32,
        alive: Arc<AtomicBool>,
        log: Arc<Mutex<Log>>,
    }
    impl Child for FakeChild {
        fn is_running(&mut self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
        fn kill(&mut self) {
            self.log.lock().unwrap().killed.push(self.appid);
        }
    }
    #[derive(Default)]
    struct FakeSpawner {
        log: Arc<Mutex<Log>>,
        alive: Mutex<Vec<(u32, Arc<AtomicBool>)>>,
    }
    impl Spawner for FakeSpawner {
        type Child = FakeChild;
        fn spawn(&self, appid: u32) -> Result<FakeChild, String> {
            self.log.lock().unwrap().spawned.push(appid);
            let alive = Arc::new(AtomicBool::new(true));
            self.alive.lock().unwrap().push((appid, alive.clone()));
            Ok(FakeChild { appid, alive, log: self.log.clone() })
        }
    }
    fn runner() -> ProcessRunner<FakeSpawner> {
        ProcessRunner::new(FakeSpawner::default())
    }
    fn spawned(runner: &ProcessRunner<FakeSpawner>) -> Vec<u32> {
        runner.spawner.log.lock().unwrap().spawned.clone()
    }
    fn killed(runner: &ProcessRunner<FakeSpawner>) -> Vec<u32> {
        runner.spawner.log.lock().unwrap().killed.clone()
    }
    #[test]
    fn applying_starts_what_is_missing_and_stops_the_rest() {
        let runner = runner();
        runner.apply(&BTreeSet::from([1, 2])).unwrap();
        runner.apply(&BTreeSet::from([2, 3])).unwrap();
        assert_eq!(spawned(&runner), vec![1, 2, 3]);
        assert_eq!(killed(&runner), vec![1]);
        assert_eq!(runner.running(), BTreeSet::from([2, 3]));
    }
    #[test]
    fn no_more_than_thirty_two_games_run() {
        let runner = runner();
        runner.apply(&(1..=40).collect()).unwrap();
        assert_eq!(spawned(&runner), (1..=32).collect::<Vec<u32>>());
    }
    #[test]
    fn a_helper_that_dies_is_noticed() {
        let runner = runner();
        runner.apply(&BTreeSet::from([1])).unwrap();
        runner.spawner.alive.lock().unwrap()[0].1.store(false, Ordering::SeqCst);
        assert_eq!(runner.running(), BTreeSet::new());
    }
    #[test]
    fn a_helper_that_died_is_started_again() {
        let runner = runner();
        runner.apply(&BTreeSet::from([1])).unwrap();
        runner.spawner.alive.lock().unwrap()[0].1.store(false, Ordering::SeqCst);
        runner.apply(&BTreeSet::from([1])).unwrap();
        assert_eq!(spawned(&runner), vec![1, 1]);
    }
    #[test]
    fn stopping_kills_everything() {
        let runner = runner();
        runner.apply(&BTreeSet::from([1, 2])).unwrap();
        runner.stop_all();
        assert_eq!(killed(&runner), vec![1, 2]);
        assert!(runner.running().is_empty());
    }
}
