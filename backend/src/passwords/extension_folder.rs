use std::path::Path;

pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &to.join(&name))?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), to.join(&name))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_copy_contains_and_updates_both_browser_builds() {
        let root = std::env::temp_dir().join(format!("myle-extension-copy-{}", std::process::id()));
        let source = root.join("bundle");
        let local = root.join("local");
        std::fs::create_dir_all(source.join("firefox/icons")).unwrap();
        std::fs::write(source.join("manifest.json"), "chromium-v1").unwrap();
        std::fs::write(source.join("firefox/manifest.json"), "firefox-v1").unwrap();
        std::fs::write(source.join("firefox/icons/32.png"), b"icon").unwrap();
        copy_tree(&source, &local).unwrap();
        assert_eq!(std::fs::read_to_string(local.join("manifest.json")).unwrap(), "chromium-v1");
        assert!(local.join("firefox/manifest.json").is_file(), "Firefox must be available in the folder offered by MYLE");
        assert_eq!(std::fs::read(local.join("firefox/icons/32.png")).unwrap(), b"icon");
        std::fs::write(local.join("keep.txt"), "user file").unwrap();
        std::fs::write(source.join("firefox/manifest.json"), "firefox-v2").unwrap();
        copy_tree(&source, &local).unwrap();
        assert_eq!(std::fs::read_to_string(local.join("firefox/manifest.json")).unwrap(), "firefox-v2");
        assert_eq!(std::fs::read_to_string(local.join("keep.txt")).unwrap(), "user file");
        std::fs::remove_dir_all(root).unwrap();
    }
}
