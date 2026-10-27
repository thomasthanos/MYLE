//! Project paths use a sibling key so older MYLE clients updating `v7`
//! cannot accidentally erase this newer section of the same private row.
use crate::github_releases::sync::{SyncedProjects, validate};
use serde_json::{Value, json};

const PROJECTS_KEY: &str = "githubReleases";

pub(super) fn read(data: Value) -> Option<Value> {
    let projects = data
        .get(PROJECTS_KEY)
        .filter(|section| section.get("projects").is_some());
    let mut snapshot = data.get(super::DATA_KEY).filter(|value|
        value.get("version").and_then(Value::as_u64) == Some(1)
        && value.get("updatedAt").is_some_and(Value::is_string)
        && value.get("storage").is_some_and(Value::is_object)).cloned()
        .or_else(|| projects.map(|section| json!({
            "version": 1, "updatedAt": section.get("updatedAt").and_then(Value::as_str).unwrap_or(""),
            "storage": {}, "gameSaves": null,
        })))?;
    if let Some(section) = projects {
        let object = snapshot.as_object_mut()?;
        object.insert(PROJECTS_KEY.into(), section["projects"].clone());
        if let Some(timestamp) = section.get("updatedAt").and_then(Value::as_str) {
            object.insert("githubReleasesUpdatedAt".into(), timestamp.into());
        }
    }
    Some(snapshot)
}

pub(super) fn write(row: Option<Value>, mut settings: Value) -> Result<Value, String> {
    let object = settings
        .as_object_mut()
        .ok_or_else(|| "Settings must be an object.".to_string())?;
    let section = object.remove(PROJECTS_KEY).filter(|value| !value.is_null());
    let projects_changed = object.remove("githubReleasesUpdatedAt");
    let mut data = row
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if let Some(section) = section {
        let projects: SyncedProjects = serde_json::from_value(section)
            .map_err(|error| format!("Invalid synced project paths: {error}"))?;
        validate(&projects)?;
        let updated_at = projects_changed
            .as_ref()
            .or_else(|| object.get("updatedAt"))
            .and_then(Value::as_str)
            .filter(|time| !time.is_empty() && time.len() <= 64)
            .ok_or_else(|| "Synced project paths need a change timestamp.".to_string())?;
        // A newer cloud list may have arrived since this PC last pulled.
        // Pushing an unrelated preference must not replace it with stale paths.
        let remote_changed = data
            .get(PROJECTS_KEY)
            .and_then(|section| section.get("updatedAt"))
            .and_then(Value::as_str);
        if !remote_changed.is_some_and(|remote| remote > updated_at) {
            data.insert(
                PROJECTS_KEY.into(),
                json!({ "updatedAt": updated_at, "projects": projects }),
            );
        }
    }
    data.insert(super::DATA_KEY.into(), settings);
    Ok(Value::Object(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Value {
        json!({"version":1,"updatedAt":"2026-10-27T11:00:00Z","storage":{},"gameSaves":null})
    }

    #[test]
    fn private_row_round_trip_keeps_other_settings_and_strips_non_path_fields() {
        let mut settings = snapshot();
        settings[PROJECTS_KEY] = json!({"version":1,"paths":["H:\\Code\\MYLE"],"token":"must not travel","buildCommand":"must not travel"});
        let data = write(Some(json!({"legacy":{"theme":"dark"}})), settings).unwrap();
        assert_eq!(data["legacy"]["theme"], "dark");
        assert!(data[super::super::DATA_KEY].get(PROJECTS_KEY).is_none());
        let restored = read(data).unwrap();
        assert_eq!(
            restored[PROJECTS_KEY],
            json!({"version":1,"paths":["H:\\Code\\MYLE"]})
        );
    }

    #[test]
    fn old_clients_keep_the_sibling_projects_when_replacing_their_v7_snapshot() {
        let mut settings = snapshot();
        settings[PROJECTS_KEY] = json!({"version":1,"paths":["C:\\Code\\Project"]});
        let initial = write(None, settings).unwrap();
        let after_old_client = write(Some(initial), snapshot()).unwrap();
        assert_eq!(
            read(after_old_client).unwrap()[PROJECTS_KEY]["paths"],
            json!(["C:\\Code\\Project"])
        );
    }

    #[test]
    fn explicit_empty_list_clears_projects_and_a_paths_only_row_can_restore() {
        let data = json!({"githubReleases":{"updatedAt":"2026-10-27T11:00:00Z","projects":{"version":1,"paths":["C:\\Code\\One"]}}});
        let mut restored = read(data.clone()).unwrap();
        assert_eq!(restored["version"], 1);
        restored[PROJECTS_KEY]["paths"] = json!([]);
        assert_eq!(
            read(write(Some(data), restored).unwrap()).unwrap()[PROJECTS_KEY]["paths"],
            json!([])
        );
    }

    #[test]
    fn invalid_incoming_paths_are_rejected_before_any_row_is_produced() {
        let mut settings = snapshot();
        settings[PROJECTS_KEY] = json!({"version":1,"paths":["relative"]});
        assert!(write(None, settings).is_err());
    }

    #[test]
    fn unrelated_preferences_do_not_change_the_project_lists_own_timestamp() {
        let mut settings = snapshot();
        settings["githubReleasesUpdatedAt"] = json!("2026-10-26T08:00:00Z");
        settings[PROJECTS_KEY] = json!({"version":1,"paths":["C:\\Code\\One"]});
        let data = write(None, settings).unwrap();
        assert_eq!(data[PROJECTS_KEY]["updatedAt"], "2026-10-26T08:00:00Z");
        let restored = read(data).unwrap();
        assert_eq!(restored["githubReleasesUpdatedAt"], "2026-10-26T08:00:00Z");
        assert_eq!(restored["updatedAt"], "2026-10-27T11:00:00Z");
    }

    #[test]
    fn pushing_preferences_with_stale_paths_preserves_a_newer_cloud_list() {
        let remote = json!({"githubReleases":{"updatedAt":"2026-10-27T10:00:00Z","projects":{"version":1,"paths":["D:\\Newer\\Repo"]}}});
        let mut local = snapshot();
        local["githubReleasesUpdatedAt"] = json!("2026-10-26T08:00:00Z");
        local[PROJECTS_KEY] = json!({"version":1,"paths":[]});
        local["storage"]["myle.dark"] = json!("1");
        let restored = read(write(Some(remote), local).unwrap()).unwrap();
        assert_eq!(restored[PROJECTS_KEY]["paths"], json!(["D:\\Newer\\Repo"]));
        assert_eq!(restored["storage"]["myle.dark"], "1");
    }

    #[test]
    fn a_project_edit_does_not_make_older_preferences_appear_newer() {
        let restored = read(json!({
            "v7":snapshot(),
            "githubReleases":{"updatedAt":"2026-10-27T12:00:00Z","projects":{"version":1,"paths":[]}}
        })).unwrap();
        assert_eq!(restored["updatedAt"], "2026-10-27T11:00:00Z");
        assert_eq!(restored["githubReleasesUpdatedAt"], "2026-10-27T12:00:00Z");
    }
}
