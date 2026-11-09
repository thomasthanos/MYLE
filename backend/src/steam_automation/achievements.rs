//! Unlocking the achievements of the games the owner picked, one at a time.
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use crate::apps::process::JobObject;
const MIN_DELAY_SECS: u64 = 5 * 60;
const MAX_DELAY_SECS: u64 = 30 * 60;
const CATEGORY_VAC: u32 = 8;
const CATEGORY_ACHIEVEMENTS: u32 = 22;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HELPER_LIMIT: Duration = Duration::from_secs(45);
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AchievementList {
    pub names: Vec<String>,
    pub locked: Vec<String>,
}
pub(crate) trait AchievementTool: Send + Sync {
    fn list(&self, appid: u32) -> impl Future<Output = Result<AchievementList, String>> + Send;
    fn unlock(&self, appid: u32, name: &str) -> impl Future<Output = Result<(), String>> + Send;
}
/// Chooses what to unlock next, and remembers what Steam would not unlock.
#[derive(Default)]
pub(crate) struct UnlockQueue {
    refused: HashSet<(u32, String)>,
}
impl UnlockQueue {
    pub(crate) fn next(&mut self, picked: &[u32], lists: &HashMap<u32, AchievementList>, random: u64) -> Option<(u32, String)> {
        let open = |appid: u32| -> Vec<&String> {
            lists
                .get(&appid)
                .map(|list| list.locked.iter().filter(|name| !self.refused.contains(&(appid, (*name).clone()))).collect())
                .unwrap_or_default()
        };
        let games: Vec<u32> = picked.iter().copied().filter(|appid| !open(*appid).is_empty()).collect();
        let appid = *games.get((random % games.len().max(1) as u64) as usize)?;
        let names = open(appid);
        let name = names[((random >> 32) % names.len() as u64) as usize].clone();
        Some((appid, name))
    }
    pub(crate) fn mark_refused(&mut self, appid: u32, name: &str) {
        self.refused.insert((appid, name.to_string()));
    }
}
pub(crate) fn next_delay_secs(random: u64) -> u64 {
    MIN_DELAY_SECS + random % (MAX_DELAY_SECS - MIN_DELAY_SECS + 1)
}
/// Whether a game can be offered, from its Steam feature categories.
pub(crate) fn eligible(feature_categories: &[u32]) -> bool {
    feature_categories.contains(&CATEGORY_ACHIEVEMENTS) && !feature_categories.contains(&CATEGORY_VAC)
}
/// Something that keeps a process from outliving MYLE.
pub(crate) trait Adopt: Send + Sync {
    fn adopt(&self, process: windows_sys::Win32::Foundation::HANDLE) -> bool;
}
impl Adopt for JobObject {
    fn adopt(&self, process: windows_sys::Win32::Foundation::HANDLE) -> bool {
        self.assign(process)
    }
}
/// Runs this same program as a short-lived helper, connected as the game.
pub(crate) struct HelperTool {
    pub job: Arc<JobObject>,
}
async fn run_helper(job: &impl Adopt, args: Vec<String>) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    run_in_job(job, &exe, &args).await
}
/// A helper outside the job could outlive MYLE, so one that cannot join does not run on.
async fn run_in_job(job: &impl Adopt, program: &std::path::Path, args: &[String]) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if !child.raw_handle().is_some_and(|process| job.adopt(process)) {
        let _ = child.kill().await;
        return Err("Could not keep a Steam helper under MYLE's control.".to_string());
    }
    let output = tokio::time::timeout(HELPER_LIMIT, child.wait_with_output())
        .await
        .map_err(|_| "Steam took too long to answer.".to_string())?
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
impl AchievementTool for HelperTool {
    async fn list(&self, appid: u32) -> Result<AchievementList, String> {
        let output = run_helper(&*self.job, vec![format!("--steam-achievements={appid}")]).await?;
        serde_json::from_str(output.trim()).map_err(|_| "Could not read the achievements.".to_string())
    }
    async fn unlock(&self, appid: u32, name: &str) -> Result<(), String> {
        run_helper(&*self.job, vec![format!("--steam-unlock={appid}"), format!("--steam-achievement={name}")]).await.map(|_| ())
    }
}
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;
    fn list(locked: &[&str]) -> AchievementList {
        AchievementList { names: locked.iter().map(|s| s.to_string()).collect(), locked: locked.iter().map(|s| s.to_string()).collect() }
    }
    #[derive(Default)]
    struct FakeJob {
        adopted: std::sync::atomic::AtomicU32,
        refuses: bool,
    }
    impl Adopt for FakeJob {
        fn adopt(&self, _process: windows_sys::Win32::Foundation::HANDLE) -> bool {
            self.adopted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            !self.refuses
        }
    }
    fn cmd() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_string()))
    }
    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    #[tokio::test]
    async fn a_helper_joins_the_job_and_gives_its_answer() {
        let job = FakeJob::default();
        let output = run_in_job(&job, &cmd(), &strings(&["/c", "echo hello"])).await.unwrap();
        assert_eq!(output.trim(), "hello");
        assert_eq!(job.adopted.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn a_helper_that_cannot_join_the_job_does_not_run_on() {
        let job = FakeJob { refuses: true, ..FakeJob::default() };
        let started = std::time::Instant::now();
        assert!(run_in_job(&job, &cmd(), &strings(&["/c", "ping -n 20 127.0.0.1"])).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(10), "it was stopped, not waited for");
    }
    #[test]
    fn the_delay_stays_between_five_and_thirty_minutes() {
        for random in [0, 1, 1500, 1501, 999_999, u64::MAX] {
            let delay = next_delay_secs(random);
            assert!((300..=1800).contains(&delay), "{delay}");
        }
    }
    #[test]
    fn the_delay_really_varies() {
        assert_ne!(next_delay_secs(0), next_delay_secs(700));
    }
    #[test]
    fn only_locked_achievements_of_picked_games_come_up() {
        let lists = HashMap::from([(1, list(&["A"])), (2, list(&["B"]))]);
        assert_eq!(UnlockQueue::default().next(&[1], &lists, 0), Some((1, "A".to_string())));
    }
    #[test]
    fn a_finished_game_is_passed_over() {
        let lists = HashMap::from([(1, list(&[])), (2, list(&["B"]))]);
        assert_eq!(UnlockQueue::default().next(&[1, 2], &lists, 0), Some((2, "B".to_string())));
    }
    #[test]
    fn nothing_left_is_nothing() {
        let lists = HashMap::from([(1, list(&[]))]);
        assert_eq!(UnlockQueue::default().next(&[1], &lists, 0), None);
    }
    #[test]
    fn a_game_with_no_list_yet_is_passed_over() {
        assert_eq!(UnlockQueue::default().next(&[1], &HashMap::new(), 0), None);
    }
    #[test]
    fn a_refused_achievement_is_not_tried_again() {
        let lists = HashMap::from([(1, list(&["A", "B"]))]);
        let mut queue = UnlockQueue::default();
        queue.mark_refused(1, "A");
        assert_eq!(queue.next(&[1], &lists, 0), Some((1, "B".to_string())));
        queue.mark_refused(1, "B");
        assert_eq!(queue.next(&[1], &lists, 0), None);
    }
    #[test]
    fn the_game_is_chosen_at_random_among_those_with_work() {
        let lists = HashMap::from([(1, list(&["A"])), (2, list(&["B"]))]);
        let mut queue = UnlockQueue::default();
        assert_eq!(queue.next(&[1, 2], &lists, 0).unwrap().0, 1);
        assert_eq!(queue.next(&[1, 2], &lists, 1).unwrap().0, 2);
    }
    #[test]
    fn vac_games_are_not_eligible() {
        assert!(!eligible(&[22, 8]));
    }
    #[test]
    fn games_without_achievements_are_not_eligible() {
        assert!(!eligible(&[29]));
        assert!(eligible(&[22, 29]));
    }
}
