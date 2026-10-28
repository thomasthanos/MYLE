use super::model::Profiles;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 2 * 1024 * 1024;
pub fn path() -> Result<PathBuf, String> {
    Ok(crate::storage::roaming_dir()?
        .join("auto-clicker")
        .join("profiles.json"))
}
pub fn load(path: &Path) -> Result<Profiles, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Profiles::default());
        }
        Err(error) => return Err(format!("Cannot read Auto Clicker profiles: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The Auto Clicker profiles file is too large.".into());
    }
    let profiles: Profiles = serde_json::from_slice(&bytes).map_err(|e| {
        format!("Cannot read Auto Clicker profiles. The original file is unchanged: {e}")
    })?;
    profiles.validate()?;
    Ok(profiles)
}
pub fn save(path: &Path, profiles: &Profiles) -> Result<(), String> {
    profiles.validate()?;
    let bytes = serde_json::to_vec_pretty(profiles).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The Auto Clicker profiles file is too large.".into());
    }
    crate::game_saves::atomic::write(path, &bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_file_defaults_but_corrupt_file_is_preserved() {
        let root = std::env::temp_dir().join(format!("myle-clicker-{}", uuid::Uuid::new_v4()));
        let path = root.join("profiles.json");
        assert!(load(&path).unwrap().validate().is_ok());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "{broken").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn atomic_roundtrip_and_invalid_replacement_preserve_profiles() {
        let root = std::env::temp_dir().join(format!("myle-clicker-{}", uuid::Uuid::new_v4()));
        let path = root.join("profiles.json");
        let profiles = Profiles::default();
        save(&path, &profiles).unwrap();
        assert_eq!(load(&path).unwrap(), profiles);
        let mut invalid = profiles.clone();
        invalid.items.clear();
        assert!(save(&path, &invalid).is_err());
        assert_eq!(load(&path).unwrap(), profiles);
        std::fs::remove_dir_all(root).unwrap();
    }
}
