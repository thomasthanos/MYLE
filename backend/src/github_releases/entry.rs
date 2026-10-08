//! A project on the page: a whole repository, or one app folder of a
//! monorepo. Its id is `<repo id>` or `<repo id>/<folder>`; the folder must
//! be one the repository really has, so an id can't point anywhere else.

use std::path::PathBuf;

use super::project;
use super::store::{EntryConfig, Repo, Settings};

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub id: String,
    pub repo_id: String,
    /// The repository's top folder.
    pub root: PathBuf,
    /// The app folder in a monorepo, with `/`.
    pub sub: Option<String>,
    /// Where the project's files are: `root` or `root/sub`.
    pub dir: PathBuf,
    pub name: String,
    pub monorepo: bool,
    pub tag_prefix: String,
    pub config: EntryConfig,
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// The projects of `repo`: one per app folder of a monorepo, else itself.
pub(crate) fn entries_of(settings: &Settings, repo: &Repo) -> Vec<Entry> {
    let root = PathBuf::from(&repo.path);
    let subs = if root.is_dir() {
        project::sub_projects(&root)
    } else {
        Vec::new()
    };
    let monorepo = subs.len() > 1;
    if subs.is_empty() {
        return vec![Entry {
            id: repo.id.clone(),
            repo_id: repo.id.clone(),
            dir: root.clone(),
            name: folder_name(&root),
            root,
            sub: None,
            monorepo: false,
            tag_prefix: "v".into(),
            config: settings.entry(&repo.id),
        }];
    }
    subs.into_iter()
        .map(|sub| {
            let id = format!("{}/{sub}", repo.id);
            Entry {
                dir: root.join(sub.replace('/', "\\")),
                name: sub.rsplit('/').next().unwrap_or(&sub).to_string(),
                tag_prefix: project::tag_prefix(Some(&sub), monorepo),
                config: settings.entry(&id),
                id,
                repo_id: repo.id.clone(),
                root: root.clone(),
                sub: Some(sub),
                monorepo,
            }
        })
        .collect()
}

pub(crate) fn resolve(settings: &Settings, id: &str) -> Result<Entry, String> {
    let repo_id = id.split('/').next().unwrap_or(id);
    let repo = settings.repo(repo_id)?;
    entries_of(settings, repo)
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| "This project's folder is no longer there.".to_string())
}

impl Entry {
    /// A path from the project folder as a path from the repository's top.
    pub(crate) fn repo_path(&self, rel: &str) -> String {
        match &self.sub {
            Some(sub) => format!("{sub}/{rel}"),
            None => rel.to_string(),
        }
    }

    /// Whether `path` (from the repository's top) is inside this project.
    pub(crate) fn contains(&self, path: &str) -> bool {
        match &self.sub {
            Some(sub) => path == sub || path.starts_with(&format!("{sub}/")),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_monorepo_gives_one_project_per_app_with_its_own_tags() {
        let dir = std::env::temp_dir().join(format!("myle-gr-entry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (app, version) in [("backup_projects", "2.0.6"), ("discord_bot", "1.0.1")] {
            std::fs::create_dir_all(dir.join(app)).unwrap();
            std::fs::write(
                dir.join(app).join("package.json"),
                format!("{{\"version\":\"{version}\"}}"),
            )
            .unwrap();
        }
        let mut settings = Settings::default();
        let id = settings.add_repo(&dir).unwrap();
        let repo = settings.repo(&id).unwrap().clone();
        let entries = entries_of(&settings, &repo);
        assert_eq!(entries.len(), 2);
        let backup = &entries[0];
        assert_eq!(backup.id, format!("{id}/backup_projects"));
        assert_eq!(backup.tag_prefix, "backup_projects-v");
        assert!(backup.monorepo);
        assert_eq!(
            backup.repo_path("package.json"),
            "backup_projects/package.json"
        );
        assert!(
            backup.contains("backup_projects/src/a.ts")
                && !backup.contains("backup_projects2/a.ts")
        );
        assert!(resolve(&settings, &format!("{id}/discord_bot")).is_ok());
        // Only folders the repository really has.
        assert!(resolve(&settings, &format!("{id}/../elsewhere")).is_err());
        assert!(resolve(&settings, &id).is_err());

        // A single project is the repository itself, tagged v….
        std::fs::write(dir.join("package.json"), "{\"version\":\"9.5.0\"}").unwrap();
        let entries = entries_of(&settings, &repo);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            (
                entries[0].id.as_str(),
                entries[0].tag_prefix.as_str(),
                entries[0].sub.is_none()
            ),
            (id.as_str(), "v", true)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
