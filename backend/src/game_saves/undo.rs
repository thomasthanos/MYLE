use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use super::models::UndoRestore;
use super::{atomic, parser};
const METADATA_FILE: &str = "latest-restore.json";
const RETENTION_SECONDS: u64 = 7 * 24 * 60 * 60;
static NEXT_SLOT: AtomicU64 = AtomicU64::new(1);
pub(crate) fn create_slot(root: &Path) -> Result<(UndoRestore, PathBuf), String> {
    let created_at = parser::now();
    prune_expired_slots(root, created_at);
    let sequence = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    let id = format!("restore-{created_at}-{}-{sequence}", std::process::id());
    let path = slot_path(root, &id)?;
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    Ok((
        UndoRestore {
            id,
            created_at,
            expires_at: created_at.saturating_add(RETENTION_SECONDS),
            games: Vec::new(),
        },
        path,
    ))
}
fn prune_expired_slots(root: &Path, now: u64) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let created = name
            .strip_prefix("restore-")
            .and_then(|rest| rest.split('-').next())
            .and_then(|value| value.parse::<u64>().ok());
        if created.is_some_and(|created| now.saturating_sub(created) >= RETENTION_SECONDS) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
pub(crate) fn commit(root: &Path, mut undo: UndoRestore, games: Vec<String>) -> Result<(), String> {
    undo.games = games;
    let previous = load_valid(root);
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let path = root.join(METADATA_FILE);
    let json = serde_json::to_vec_pretty(&undo).map_err(|error| error.to_string())?;
    atomic::write(&path, &json)?;
    if let Some(previous) = previous.filter(|previous| previous.id != undo.id)
        && let Ok(old_path) = slot_path(root, &previous.id)
    {
        let _ = fs::remove_dir_all(old_path);
    }
    Ok(())
}
pub(crate) fn load_valid(root: &Path) -> Option<UndoRestore> {
    let path = root.join(METADATA_FILE);
    let undo: UndoRestore = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())?;
    let slot = slot_path(root, &undo.id).ok()?;
    if undo.expires_at <= parser::now() || !slot.is_dir() || undo.games.is_empty() {
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(slot);
        return None;
    }
    Some(undo)
}
/// Forgets the last restore's safety copy: a newer restore that changes
/// nothing must not leave "Undo last restore" putting back an older one.
pub(crate) fn clear(root: &Path) {
    if let Some(previous) = load_valid(root) {
        let _ = fs::remove_file(root.join(METADATA_FILE));
        discard_slot(root, &previous.id);
    }
}
pub(crate) fn discard_slot(root: &Path, id: &str) {
    if let Ok(path) = slot_path(root, id) {
        let _ = fs::remove_dir_all(path);
    }
}
pub(crate) fn slot_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !id.starts_with("restore-")
        || id.len() > 96
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("Invalid restore safety-copy identifier.".into());
    }
    Ok(root.join(id))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slot_ids_cannot_escape_the_safety_root() {
        let root = Path::new(r"C:\safe");
        assert!(slot_path(root, "restore-123-9").is_ok());
        assert!(slot_path(root, r"..\outside").is_err());
        assert!(slot_path(root, "restore-a/b").is_err());
    }
}
