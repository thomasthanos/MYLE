use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupSchedule {
    #[default]
    Off,
    Daily,
    Weekly,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScheduleWeekday {
    #[default]
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}

impl BackupSchedule {
    pub(crate) fn interval_seconds(self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::Daily => Some(24 * 60 * 60),
            Self::Weekly => Some(7 * 24 * 60 * 60),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootStore {
    Steam,
    Epic,
    Gog,
    GogGalaxy,
    Uplay,
    Origin,
    OtherWindows,
}

impl RootStore {
    pub(crate) fn ludusavi_name(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Epic => "epic",
            Self::Gog => "gog",
            Self::GogGalaxy => "gogGalaxy",
            Self::Uplay => "uplay",
            Self::Origin => "origin",
            // Ludusavi's `otherWindows` root represents a mounted offline
            // Windows installation. The UI option is an ordinary extra game
            // folder, whose engine store identifier is `other`.
            Self::OtherWindows => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootSource {
    Automatic,
    Manual,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRoot {
    pub id: String,
    pub path: String,
    pub store: RootStore,
    pub source: RootSource,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomGame {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default = "default_true")]
    pub auto_backup: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSavesSettings {
    #[serde(default = "settings_version")]
    pub version: u32,
    #[serde(default)]
    pub backup_folder: Option<String>,
    #[serde(default)]
    pub schedule: BackupSchedule,
    #[serde(default = "default_schedule_time")]
    pub schedule_time: String,
    #[serde(default)]
    pub schedule_weekday: ScheduleWeekday,
    #[serde(default)]
    pub last_scheduled_attempt: Option<u64>,
    #[serde(default)]
    pub last_scheduled_success: Option<u64>,
    #[serde(default)]
    pub last_scheduled_result: Option<ScheduledBackupResult>,
    #[serde(default)]
    pub roots: Vec<GameRoot>,
    #[serde(default)]
    pub auto_backup_excluded_game_ids: Vec<String>,
    #[serde(default)]
    pub custom_games: Vec<CustomGame>,
    #[serde(default)]
    pub path_mappings: Vec<RestorePathMapping>,
    #[serde(default)]
    pub database_games: u64,
    #[serde(default)]
    pub database_updated_at: Option<u64>,
}

fn default_schedule_time() -> String {
    "03:00".into()
}

pub(crate) const GAME_SAVES_SETTINGS_VERSION: u32 = 1;

const fn settings_version() -> u32 {
    GAME_SAVES_SETTINGS_VERSION
}

impl Default for GameSavesSettings {
    fn default() -> Self {
        Self {
            version: settings_version(),
            backup_folder: None,
            schedule: BackupSchedule::Off,
            schedule_time: default_schedule_time(),
            schedule_weekday: ScheduleWeekday::Sunday,
            last_scheduled_attempt: None,
            last_scheduled_success: None,
            last_scheduled_result: None,
            roots: Vec::new(),
            auto_backup_excluded_game_ids: Vec::new(),
            custom_games: Vec::new(),
            path_mappings: Vec::new(),
            database_games: 0,
            database_updated_at: None,
        }
    }
}

/// The part of the Game Saves settings that follows the user's account to
/// other PCs. Folders (backup, game libraries, restore locations) belong to
/// one machine and stay out.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedGameSaves {
    pub schedule: BackupSchedule,
    pub schedule_time: String,
    pub schedule_weekday: ScheduleWeekday,
    #[serde(default)]
    pub auto_backup_excluded_game_ids: Vec<String>,
    #[serde(default)]
    pub custom_games: Vec<CustomGame>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledBackupResult {
    pub attempted_at: u64,
    pub completed_at: u64,
    pub processed_games: u64,
    pub failed_games: Vec<String>,
    pub error: Option<String>,
}

pub use crate::cloud::CloudProvider;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedFolder {
    pub provider: CloudProvider,
    /// Which account, when a provider has several ("Dropbox Business").
    pub label: String,
    /// The provider's synced folder.
    pub path: String,
    /// Where backups go inside it (created when chosen).
    pub backup_path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Scan,
    UpdateDatabase,
    Backup,
    Restore,
    ScheduledBackup,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationStage {
    Preparing,
    Scanning,
    UpdatingDatabase,
    CreatingSafetyBackup,
    BackingUp,
    /// Waiting for OneDrive to start and download online-only saves.
    WaitingForOneDrive,
    Restoring,
    Finishing,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum GameSavesEvent {
    Stage {
        stage: OperationStage,
    },
    Progress {
        done: usize,
        total: usize,
        current: Option<String>,
    },
    Message {
        text: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GameSaveStatus {
    NotBackedUp,
    BackedUp,
    ChangedSinceBackup,
    BackupOnly,
    NeedsLocation,
    Unknown,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePathMapping {
    pub game_id: String,
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSaveEntry {
    pub id: String,
    pub title: String,
    pub status: GameSaveStatus,
    pub platform_badges: Vec<String>,
    pub file_count: u64,
    pub total_bytes: u64,
    pub last_save_at: Option<u64>,
    pub last_backup_at: Option<String>,
    pub paths: Vec<String>,
    pub auto_backup: bool,
    pub has_local_data: bool,
    pub has_backup: bool,
    pub error: Option<String>,
    pub snapshots: Vec<GameSaveSnapshot>,
    /// Its Steam app id in the save database, for its cover.
    #[serde(default)]
    pub steam_id: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSaveSnapshot {
    /// Ludusavi's stable backup name, accepted again by `restore --backup`.
    pub id: String,
    /// RFC 3339 timestamp supplied by the backup engine.
    pub timestamp: String,
    pub bytes: u64,
    pub label: Option<String>,
    pub is_safety: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSelection {
    pub game_id: String,
    pub snapshot_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSavesStats {
    pub local_games: usize,
    pub backup_games: usize,
    pub total_bytes: u64,
    pub database_games: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSavesScan {
    pub generated_at: u64,
    pub stats: GameSavesStats,
    pub on_this_pc: Vec<GameSaveEntry>,
    pub in_backup: Vec<GameSaveEntry>,
    /// When every game in the database was last looked for. A quick refresh
    /// only re-checks the games already known and keeps this time.
    #[serde(default)]
    pub full_scan_at: u64,
    /// This result came from a quick refresh of the known games.
    #[serde(default)]
    pub quick: bool,
    /// Newly installed games may be missing: the game folders or custom games
    /// changed since the last full scan, or it is over a day old.
    #[serde(default)]
    pub discovery_due: bool,
    /// The backup folder, when its drive is not connected (Google Drive not
    /// running): the saves on this PC are listed, but nothing is known about
    /// their backups.
    #[serde(default)]
    pub backup_unreachable: Option<String>,
}

impl GameSavesScan {
    /// With the backup folder out of reach, no game can be said to have no
    /// backup: its status is not known until the folder is back.
    pub fn without_backups(&mut self, folder: &str) {
        for game in &mut self.on_this_pc {
            if game.status == GameSaveStatus::NotBackedUp {
                game.status = GameSaveStatus::Unknown;
            }
        }
        self.backup_unreachable = Some(folder.to_string());
    }
}

/// How much a scan looks at: only the games already found (seconds), or
/// every game in the database (close to a minute with large libraries).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanMode {
    Quick,
    #[default]
    Full,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSavesOperationResult {
    pub kind: OperationKind,
    pub processed_games: u64,
    pub processed_bytes: u64,
    pub failed_games: Vec<String>,
    /// Why each failed game failed, as far as the engine said.
    #[serde(default)]
    pub failures: Vec<GameFailure>,
    pub safety_backup_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFailure {
    pub game: String,
    pub reason: String,
    /// The end of the path of the file it happened to, when there is one.
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseUpdate {
    pub games: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSavesPageState {
    pub settings: GameSavesSettings,
    pub cloud_folders: Vec<DetectedFolder>,
    pub engine_available: bool,
    pub engine_version: Option<String>,
    pub active_operation: Option<OperationKind>,
    pub database_games: u64,
    pub database_updated_at: Option<u64>,
    pub undo_restore: Option<UndoRestore>,
    /// The last scan saved on disk, shown while a fresh one runs.
    pub cached_scan: Option<GameSavesScan>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoRestore {
    /// Opaque identifier validated by the backend before it is used.
    pub id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub games: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(status: GameSaveStatus) -> GameSaveEntry {
        GameSaveEntry {
            id: "game".into(),
            title: "Game".into(),
            status,
            platform_badges: Vec::new(),
            file_count: 1,
            total_bytes: 1,
            last_save_at: None,
            last_backup_at: None,
            paths: Vec::new(),
            auto_backup: true,
            has_local_data: true,
            has_backup: false,
            error: None,
            snapshots: Vec::new(),
            steam_id: None,
        }
    }

    #[test]
    fn without_its_backups_no_game_is_said_to_have_none() {
        let mut scan = GameSavesScan {
            on_this_pc: vec![game(GameSaveStatus::NotBackedUp), game(GameSaveStatus::Error)],
            ..Default::default()
        };
        scan.without_backups(r"G:\My Drive\Backups");
        assert_eq!(scan.on_this_pc[0].status, GameSaveStatus::Unknown);
        assert_eq!(scan.on_this_pc[1].status, GameSaveStatus::Error);
        assert_eq!(scan.backup_unreachable.as_deref(), Some(r"G:\My Drive\Backups"));
    }
}
