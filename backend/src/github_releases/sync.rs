//! Only repository paths travel with the account. No commands or credentials.
use super::store::Settings;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, Prefix};
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedProjects {
    pub version: u32,
    pub paths: Vec<String>,
}
fn key(path: &str) -> Result<String, String> {
    if path.len() > 32767
        || path.chars().any(char::is_control)
        || path
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | ".."))
    {
        return Err("A synced project path is invalid.".into());
    }
    let mut parts = Path::new(path).components();
    let allowed_prefix = matches!(parts.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_) | Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _)));
    if !allowed_prefix || !matches!(parts.next(), Some(Component::RootDir)) {
        return Err("Synced project paths must be absolute Windows folders.".into());
    }
    let mut has_folder = false;
    for part in parts {
        if !matches!(part, Component::Normal(name) if !name.to_string_lossy().contains([':', '*', '?', '"', '<', '>', '|']))
        {
            return Err("A synced project path contains invalid components.".into());
        }
        has_folder = true;
    }
    if !has_folder {
        return Err("A whole drive or share is not a project folder.".into());
    }
    let normalized = path.replace('/', "\\").to_lowercase();
    let normalized = normalized
        .strip_prefix(r"\\?\unc\")
        .map(|p| format!(r"\\{p}"))
        .unwrap_or_else(|| {
            normalized
                .strip_prefix(r"\\?\")
                .unwrap_or(&normalized)
                .to_string()
        });
    Ok(normalized.trim_end_matches('\\').to_string())
}
/// A folder on a network share (`\\server\share\…`). Such a project stays
/// on the PC that added it: from the account, its path would make this PC
/// connect (and sign in) to whatever server it names, and run git there.
fn on_a_share(path: &str) -> bool {
    matches!(Path::new(path).components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _)))
}
pub(crate) fn validate(synced: &SyncedProjects) -> Result<(), String> {
    if synced.version != 1 {
        return Err("This project-path sync version is not supported.".into());
    }
    if synced.paths.len() > 300 {
        return Err("At most 300 project paths can be synced.".into());
    }
    for path in &synced.paths {
        key(path)?;
    }
    Ok(())
}
pub(crate) fn export(settings: &Settings) -> Result<SyncedProjects, String> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for repo in settings.repos.iter().filter(|repo| !on_a_share(&repo.path)) {
        if seen.insert(key(&repo.path)?) {
            paths.push(repo.path.clone());
        }
    }
    let synced = SyncedProjects { version: 1, paths };
    validate(&synced)?;
    Ok(synced)
}
/// Reconcile without touching any project folder. Missing paths are retained
/// for another PC/reinstall; only the ordinary page refresh inspects them.
pub(crate) fn reconcile(settings: &mut Settings, synced: &SyncedProjects) -> Result<(), String> {
    validate(synced)?;
    // Paths on a share (from an older MYLE) are not taken from the account.
    let incoming: Vec<&String> = synced.paths.iter().filter(|path| !on_a_share(path)).collect();
    let wanted: HashSet<_> = incoming
        .iter()
        .map(|path| key(path))
        .collect::<Result<_, _>>()?;
    let mut next = settings.clone();
    let removed: Vec<_> = next
        .repos
        .iter()
        // This PC's projects on a share are its own: the account never lists them.
        .filter(|repo| !on_a_share(&repo.path) && !key(&repo.path).is_ok_and(|path| wanted.contains(&path)))
        .map(|repo| repo.id.clone())
        .collect();
    for id in removed {
        next.remove_repo(&id);
    }
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for path in incoming {
        let path_key = key(path)?;
        if !seen.insert(path_key.clone()) {
            continue;
        }
        if let Some(repo) = next
            .repos
            .iter()
            .find(|repo| key(&repo.path).is_ok_and(|known| known == path_key))
        {
            ordered.push(repo.clone());
        } else {
            let id = next.add_repo(Path::new(path))?;
            ordered.push(next.repo(&id)?.clone());
        }
    }
    ordered.extend(next.repos.iter().filter(|repo| on_a_share(&repo.path)).cloned());
    next.repos = ordered;
    *settings = next;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::super::store::{Account, EntryConfig};
    use super::*;
    fn synced(paths: &[&str]) -> SyncedProjects {
        SyncedProjects {
            version: 1,
            paths: paths.iter().map(|path| (*path).into()).collect(),
        }
    }
    #[test]
    fn projects_on_a_share_stay_on_their_pc() {
        let mut settings = Settings::default();
        settings.add_repo(Path::new(r"C:\Code\App")).unwrap();
        settings.add_repo(Path::new(r"\\nas\code\Shared")).unwrap();
        assert_eq!(export(&settings).unwrap().paths, [r"C:\Code\App"], "a share never goes to the account");
        // The account's list (no share) keeps this PC's share project, and one sent by an older MYLE is not taken.
        reconcile(&mut settings, &synced(&[r"C:\Code\App", r"\\evil\share\repo"])).unwrap();
        let paths: Vec<&str> = settings.repos.iter().map(|repo| repo.path.as_str()).collect();
        assert_eq!(paths, [r"C:\Code\App", r"\\nas\code\Shared"]);
    }
    #[test]
    fn paths_restore_without_accessing_folders_and_keep_local_ids_and_commands() {
        let mut settings = Settings::default();
        let kept = settings.add_repo(Path::new(r"H:\Projects\MYLE")).unwrap();
        let removed = settings.add_repo(Path::new(r"C:\Old")).unwrap();
        settings.entries.insert(
            kept.clone(),
            EntryConfig {
                build_command: Some("local command".into()),
                ..Default::default()
            },
        );
        settings
            .entries
            .insert(format!("{removed}/app"), EntryConfig::default());
        settings.account = Some(Account {
            login: "local-account".into(),
            ..Default::default()
        });
        let ai = settings.ai.clone();
        reconcile(
            &mut settings,
            &synced(&["h:/projects/myle/", r"Z:\Missing\Repo"]),
        )
        .unwrap();
        assert_eq!(settings.repos.len(), 2);
        assert_eq!(settings.repos[0].id, kept);
        assert_eq!(settings.repos[1].path, r"Z:\Missing\Repo");
        assert_eq!(
            settings.entries[&kept].build_command.as_deref(),
            Some("local command")
        );
        assert!(!settings.entries.contains_key(&format!("{removed}/app")));
        assert_eq!(settings.account.unwrap().login, "local-account");
        assert_eq!(settings.ai, ai);
    }
    #[test]
    fn empty_cloud_list_explicitly_removes_projects_but_not_account_settings() {
        let mut settings = Settings::default();
        settings.add_repo(Path::new(r"C:\Code\One")).unwrap();
        reconcile(&mut settings, &synced(&[])).unwrap();
        assert!(settings.repos.is_empty());
    }
    #[test]
    fn paths_are_deduplicated_and_export_contains_no_local_configuration() {
        let mut settings = Settings::default();
        reconcile(
            &mut settings,
            &synced(&[r"C:\Code\One", "c:/code/one/", r"\\?\C:\Code\One"]),
        )
        .unwrap();
        assert_eq!(settings.repos.len(), 1);
        let value = serde_json::to_value(export(&settings).unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert_eq!(value["paths"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn malformed_cloud_input_never_partially_changes_settings() {
        for path in [
            "relative",
            r"C:\",
            r"C:\Code\..\other",
            "C:\\bad\0path",
            r"\\.\C:\code",
            r"C:\Code\file:stream",
        ] {
            let mut settings = Settings::default();
            settings.add_repo(Path::new(r"C:\Existing")).unwrap();
            let before = settings.clone();
            assert!(
                reconcile(&mut settings, &synced(&[r"C:\Valid", path])).is_err(),
                "accepted {path:?}"
            );
            assert_eq!(settings, before);
        }
        let mut bad = synced(&[r"C:\Code"]);
        bad.version = 2;
        assert!(validate(&bad).is_err());
        bad.version = 1;
        bad.paths = vec![r"C:\Code".into(); 301];
        assert!(validate(&bad).is_err());
    }
}
