// ! System Cleaner: measures and removes the usual Windows leftovers. ! ! The page sends category ids only. Paths, patterns and the elevated script ! all come from the fixed table in `targets.rs`.
pub mod downloads;
pub mod health;
mod elevated;
mod recycle_bin;
mod targets;
pub use elevated::run_helper_from_args as run_elevated_helper_from_args;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use crate::apps::Jobs;
use targets::{Category, Kind};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInfo {
    id: String,
    title: String,
    description: String,
    hint: String,
    icon: String,
    /// At least one folder of this category is handled by the administrator pass.
    may_need_admin: bool,
    /// The category has no user folders to scan before administrator approval.
    admin_only: bool,
}
/// Static card data, so the page can draw before any scanning happens.
#[tauri::command]
pub fn cleaner_categories() -> Vec<CategoryInfo> {
    targets::CATEGORIES
        .iter()
        .map(|c| CategoryInfo {
            id: c.id.into(),
            title: c.title.into(),
            description: c.description.into(),
            hint: c.hint.into(),
            icon: c.icon.into(),
            may_need_admin: c.targets().iter().any(|t| t.admin),
            admin_only: !c.targets().is_empty() && c.targets().iter().all(|t| t.admin),
        })
        .collect()
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum ScanEvent {
    /// One category finished measuring.
    Category {
        id: String,
        bytes: u64,
        files: u64,
        locked: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CleanEvent {
    Progress {
        done: usize,
        total: usize,
        current: String,
    },
    Category {
        id: String,
        bytes: u64,
        files: u64,
        skipped: u64,
        /// What the skipped files hold: the page says "still in use" in bytes.
        skipped_bytes: u64,
        /// Of `skipped`, those the administrator pass tries again.
        admin_skipped: u64,
        /// Of `skipped_bytes`, the part the administrator pass tries again.
        admin_skipped_bytes: u64,
        locked: bool,
    },
}
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    bytes: u64,
    /// Categories with folders excluded from the scan.
    locked: Vec<String>,
}
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanSummary {
    freed: u64,
    files: u64,
    skipped: u64,
    /// What the skipped files hold; after the administrator pass this is only
    /// what is truly in use.
    skipped_bytes: u64,
    /// Of `skipped`, those in administrator folders: the administrator pass
    /// tries them again and counts what it still leaves.
    admin_skipped: u64,
    /// Of `skipped_bytes`, the part the administrator pass tries again.
    admin_skipped_bytes: u64,
    /// Categories with folders excluded from the cleanup pass.
    locked: Vec<String>,
}
fn pick(ids: &[String]) -> Vec<&'static Category> {
    ids.iter().filter_map(|id| targets::find(id)).collect()
}
// --------------------------------------------------------------------------- Scan
#[tauri::command]
pub async fn cleaner_scan(on_event: Channel<ScanEvent>) -> Result<ScanSummary, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let mut summary = ScanSummary::default();
        for category in targets::CATEGORIES {
            let measured = measure(category);
            summary.bytes += measured.bytes;
            if measured.locked {
                summary.locked.push(category.id.into());
            }
            let _ = on_event.send(ScanEvent::Category {
                id: category.id.into(),
                bytes: measured.bytes,
                files: measured.files,
                locked: measured.locked,
            });
        }
        summary
    });
    handle.await.map_err(|e| e.to_string())
}
/// Whether the administrator helper approved earlier is still running, so the
/// next scan or clean will not ask again.
#[tauri::command]
pub async fn cleaner_admin_ready() -> bool {
    elevated::is_running().await
}
/// Measures only the system folders. The caller adds these figures to its
/// existing user scan. The helper asks for approval the first time it starts.
#[tauri::command]
pub async fn cleaner_scan_elevated(on_event: Channel<ScanEvent>) -> Result<ScanSummary, String> {
    let locked: Vec<&'static Category> = targets::CATEGORIES
        .iter()
        .filter(|c| c.targets().iter().any(|t| t.admin))
        .collect();
    let totals = elevated::run(&locked, elevated::Action::Measure).await?;
    let mut summary = ScanSummary::default();
    for category in locked {
        let measured = totals.get(category.id).copied().unwrap_or_default();
        summary.bytes += measured.bytes;
        let _ = on_event.send(ScanEvent::Category {
            id: category.id.into(),
            bytes: measured.bytes,
            files: measured.files,
            locked: false,
        });
    }
    Ok(summary)
}
fn measure(category: &Category) -> targets::Measured {
    match category.kind {
        Kind::RecycleBin => {
            let (bytes, files) = recycle_bin::query().unwrap_or_default();
            targets::Measured {
                bytes,
                files,
                locked: false,
            }
        }
        Kind::Folders(list) => list
            .iter()
            .fold(targets::Measured::default(), |mut acc, target| {
                if target.admin {
                    acc.locked = true;
                    return acc;
                }
                let one = targets::measure(target);
                acc.bytes += one.bytes;
                acc.files += one.files;
                acc.locked |= one.locked;
                acc
            }),
    }
}
// --------------------------------------------------------------------------- Clean
#[tauri::command]
pub async fn cleaner_clean(
    jobs: State<'_, Jobs>,
    ids: Vec<String>,
    on_event: Channel<CleanEvent>,
) -> Result<CleanSummary, String> {
    // winget and our own downloads keep installers in %TEMP% between the download and the install; emptying it then breaks the install. Held until the cleaning ends, so no install can start meanwhile either.
    let reservation = reserve(&jobs)?;
    let categories = pick(&ids);
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let _reservation = reservation;
        let total = categories.len();
        let mut summary = CleanSummary::default();
        for (index, category) in categories.iter().enumerate() {
            let _ = on_event.send(CleanEvent::Progress {
                done: index,
                total,
                current: category.title.into(),
            });
            let cleaned = clean(category);
            summary.freed += cleaned.bytes;
            summary.files += cleaned.files;
            summary.skipped += cleaned.skipped;
            summary.skipped_bytes += cleaned.skipped_bytes;
            summary.admin_skipped += cleaned.admin_skipped;
            summary.admin_skipped_bytes += cleaned.admin_skipped_bytes;
            if cleaned.locked {
                summary.locked.push(category.id.into());
            }
            let _ = on_event.send(CleanEvent::Category {
                id: category.id.into(),
                bytes: cleaned.bytes,
                files: cleaned.files,
                skipped: cleaned.skipped,
                skipped_bytes: cleaned.skipped_bytes,
                admin_skipped: cleaned.admin_skipped,
                admin_skipped_bytes: cleaned.admin_skipped_bytes,
                locked: cleaned.locked,
            });
        }
        let _ = on_event.send(CleanEvent::Progress {
            done: total,
            total,
            current: String::new(),
        });
        summary
    });
    handle.await.map_err(|e| e.to_string())
}
/// Removes what only an administrator can remove, in one UAC prompt.
#[tauri::command]
pub async fn cleaner_clean_elevated(
    jobs: State<'_, Jobs>,
    ids: Vec<String>,
    on_event: Channel<CleanEvent>,
) -> Result<CleanSummary, String> {
    let _reservation = reserve(&jobs)?;
    let categories: Vec<&'static Category> = pick(&ids)
        .into_iter()
        .filter(|c| c.targets().iter().any(|t| t.admin))
        .collect();
    let totals = elevated::run(&categories, elevated::Action::Clean).await?;
    let mut summary = CleanSummary::default();
    for category in categories {
        let entry = totals.get(category.id).copied().unwrap_or_default();
        summary.freed += entry.bytes;
        summary.files += entry.files;
        summary.skipped += entry.skipped;
        summary.skipped_bytes += entry.skipped_bytes;
        let _ = on_event.send(CleanEvent::Category {
            id: category.id.into(),
            bytes: entry.bytes,
            files: entry.files,
            skipped: entry.skipped,
            skipped_bytes: entry.skipped_bytes,
            admin_skipped: 0,
            admin_skipped_bytes: 0,
            locked: false,
        });
    }
    Ok(summary)
}
/// The whole job registry, for as long as the cleaning takes: no app install
/// or update is running, and none can start until this is dropped.
fn reserve(jobs: &Jobs) -> Result<crate::apps::JobHandle, String> {
    jobs.start_exclusive(&format!("system-cleaner-{}", uuid::Uuid::new_v4().simple()))
        .map_err(|_| "Wait for the running app installs or updates to finish, then clean.".to_string())
}
fn clean(category: &Category) -> targets::Cleaned {
    match category.kind {
        Kind::RecycleBin => {
            let (bytes, files) = recycle_bin::query().unwrap_or_default();
            match recycle_bin::empty() {
                Ok(()) => targets::Cleaned {
                    bytes,
                    files,
                    ..Default::default()
                },
                Err(_) => targets::Cleaned {
                    skipped: files,
                    skipped_bytes: bytes,
                    ..Default::default()
                },
            }
        }
        Kind::Folders(list) => list
            .iter()
            .fold(targets::Cleaned::default(), |mut acc, target| {
                if target.admin {
                    acc.locked = true;
                    return acc;
                }
                let one = targets::clean(target);
                acc.bytes += one.bytes;
                acc.files += one.files;
                acc.skipped += one.skipped;
                acc.skipped_bytes += one.skipped_bytes;
                acc.locked |= one.locked;
                acc
            }),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_install_starts_while_cleaning_and_none_is_cut_short() {
        let jobs = Jobs::default();
        let cleaning = reserve(&jobs).unwrap();
        assert!(jobs.start("some-app").is_err());
        drop(cleaning);
        let install = jobs.start("some-app").unwrap();
        assert!(reserve(&jobs).is_err());
        drop(install);
        assert!(reserve(&jobs).is_ok());
    }
    #[test]
    fn cards_describe_every_category_and_flag_the_admin_ones() {
        let cards = cleaner_categories();
        assert_eq!(cards.len(), targets::CATEGORIES.len());
        let by_id = |id: &str| cards.iter().find(|c| c.id == id).unwrap();
        assert!(by_id("prefetch").admin_only);
        assert!(by_id("windows-update").admin_only);
        assert!(!by_id("temp").admin_only);
        assert!(!by_id("errors").admin_only);
        assert!(by_id("errors").may_need_admin);
        assert!(!by_id("recycle-bin").admin_only);
        assert!(
            by_id("temp").may_need_admin,
            "C:\\Windows\\Temp needs rights"
        );
        assert!(!by_id("recycle-bin").may_need_admin);
        assert!(!by_id("thumbnails").may_need_admin);
    }
    #[test]
    fn standard_pass_leaves_system_only_targets_to_the_admin_helper() {
        let dir = crate::test_dirs::temp_dir("cleaner-admin-only");
        let file = dir.join("test.tmp");
        std::fs::write(&file, b"data").unwrap();
        let path = Box::leak(dir.to_str().unwrap().to_owned().into_boxed_str());
        let list = Box::leak(Box::new([targets::Target {
            dir: path,
            patterns: &[],
            admin: true,
        }]));
        let category = Category {
            kind: Kind::Folders(list),
            ..*targets::find("windows-update").unwrap()
        };
        assert_eq!(measure(&category), targets::Measured { locked: true, ..Default::default() });
        assert_eq!(clean(&category), targets::Cleaned { locked: true, ..Default::default() });
        assert!(file.exists());
    }
    #[test]
    fn unknown_ids_are_dropped_rather_than_guessed() {
        assert_eq!(pick(&["temp".into(), "nope".into()]).len(), 1);
        assert!(pick(&[r"..\..\Windows".into()]).is_empty());
    }
    #[test]
    fn events_serialize_for_the_page() {
        let json = serde_json::to_value(ScanEvent::Category {
            id: "temp".into(),
            bytes: 10,
            files: 2,
            locked: true,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "event": "category", "data": { "id": "temp", "bytes": 10, "files": 2, "locked": true } })
        );
        let cleaned = serde_json::to_value(CleanEvent::Category {
            id: "temp".into(),
            bytes: 10,
            files: 2,
            skipped: 3,
            skipped_bytes: 2048,
            admin_skipped: 1,
            admin_skipped_bytes: 1024,
            locked: false,
        })
        .unwrap();
        assert_eq!(cleaned["data"]["adminSkipped"], 1);
        assert_eq!(cleaned["data"]["skippedBytes"], 2048);
        assert_eq!(cleaned["data"]["adminSkippedBytes"], 1024);
        let summary = serde_json::to_value(CleanSummary {
            admin_skipped: 4,
            skipped_bytes: 512,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(summary["adminSkipped"], 4);
        assert_eq!(summary["skippedBytes"], 512);
    }
}
