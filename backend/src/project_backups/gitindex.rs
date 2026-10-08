//! The paths git tracks in a project, read straight from `.git/index`
//! (versions 2–4), so a `build` folder can be told apart from build output
//! without running git.

use std::fs;
use std::path::{Path, PathBuf};

/// The tracked paths of the repository whose work tree is `root`, with `/`
/// separators, or `None` when `root` is not a git work tree (or its index
/// can't be read).
pub(crate) fn tracked_paths(root: &Path) -> Option<Vec<String>> {
    let git_dir = git_dir(root)?;
    let bytes = fs::read(git_dir.join("index")).ok()?;
    parse_index(&bytes, hash_size(&git_dir))
}

/// `.git` is a folder, or (worktrees, submodules) a file naming one.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let meta = fs::metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git);
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))?
        .trim();
    let path = Path::new(target);
    let dir = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    dir.is_dir().then_some(dir)
}

/// SHA-256 repositories say so in their config.
fn hash_size(git_dir: &Path) -> usize {
    let config = fs::read_to_string(git_dir.join("config")).unwrap_or_default();
    let sha256 = config.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase().replace(' ', "");
        line == "objectformat=sha256"
    });
    if sha256 { 32 } else { 20 }
}

pub(crate) fn parse_index(bytes: &[u8], hash_size: usize) -> Option<Vec<String>> {
    if bytes.len() < 12 || &bytes[..4] != b"DIRC" {
        return None;
    }
    let version = u32::from_be_bytes(bytes[4..8].try_into().ok()?);
    if !(2..=4).contains(&version) {
        return None;
    }
    let count = u32::from_be_bytes(bytes[8..12].try_into().ok()?) as usize;
    let mut paths = Vec::with_capacity(count.min(1 << 20));
    let mut at = 12usize;
    let mut previous: Vec<u8> = Vec::new();
    for _ in 0..count {
        let start = at;
        // ctime, mtime (8 each), dev, ino, mode, uid, gid, size (4 each), the hash.
        at = at.checked_add(40 + hash_size)?;
        let flags = u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?);
        at += 2;
        if version >= 3 && flags & 0x4000 != 0 {
            at += 2;
        }
        let path = if version == 4 {
            let (strip, used) = read_offset(bytes.get(at..)?)?;
            at += used;
            let end = at + bytes.get(at..)?.iter().position(|&b| b == 0)?;
            let keep = previous.len().checked_sub(strip)?;
            let mut path = previous[..keep].to_vec();
            path.extend_from_slice(&bytes[at..end]);
            at = end + 1;
            path
        } else {
            let end = at + bytes.get(at..)?.iter().position(|&b| b == 0)?;
            let path = bytes[at..end].to_vec();
            // Entries are padded with 1–8 NULs to a multiple of eight bytes.
            let length = end - start;
            at = start + ((length + 8) & !7);
            path
        };
        paths.push(String::from_utf8_lossy(&path).into_owned());
        previous = path;
    }
    Some(paths)
}

/// Git's variable-length offset (`varint.c`).
fn read_offset(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut used = 0;
    let mut byte = *bytes.first()?;
    used += 1;
    let mut value = (byte & 0x7f) as usize;
    while byte & 0x80 != 0 {
        byte = *bytes.get(used)?;
        used += 1;
        value = value.checked_add(1)?.checked_shl(7)? | (byte & 0x7f) as usize;
    }
    Some((value, used))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A version 2 index with these paths (no extended flags), as git writes it.
    pub(crate) fn index_v2(paths: &[&str]) -> Vec<u8> {
        let mut out = b"DIRC".to_vec();
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&(paths.len() as u32).to_be_bytes());
        for path in paths {
            let start = out.len();
            out.extend_from_slice(&[0u8; 40]);
            out.extend_from_slice(&[0xab; 20]);
            out.extend_from_slice(&(path.len().min(0xfff) as u16).to_be_bytes());
            out.extend_from_slice(path.as_bytes());
            let length = out.len() - start;
            let padded = (length + 8) & !7;
            out.resize(start + padded, 0);
        }
        out.extend_from_slice(&[0u8; 20]); // the checksum (not read)
        out
    }

    fn index_v4(paths: &[&str]) -> Vec<u8> {
        let mut out = b"DIRC".to_vec();
        out.extend_from_slice(&4u32.to_be_bytes());
        out.extend_from_slice(&(paths.len() as u32).to_be_bytes());
        let mut previous = "";
        for path in paths {
            out.extend_from_slice(&[0u8; 40]);
            out.extend_from_slice(&[0xab; 20]);
            out.extend_from_slice(&(path.len() as u16).to_be_bytes());
            let common = previous
                .bytes()
                .zip(path.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let strip = previous.len() - common;
            assert!(strip < 0x80, "the test only writes one-byte offsets");
            out.push(strip as u8);
            out.extend_from_slice(&path.as_bytes()[common..]);
            out.push(0);
            previous = path;
        }
        out
    }

    #[test]
    fn version_two_and_four_indexes_are_read() {
        let paths = [
            "README.md",
            "build/installer.nsh",
            "src/main.rs",
            "src/nested/deep file.txt",
        ];
        assert_eq!(parse_index(&index_v2(&paths), 20).unwrap(), paths);
        assert_eq!(parse_index(&index_v4(&paths), 20).unwrap(), paths);
        assert_eq!(parse_index(b"not an index", 20), None);
        // A truncated index is refused rather than half read.
        let full = index_v2(&paths);
        assert_eq!(parse_index(&full[..40], 20), None);
    }

    #[test]
    fn a_git_file_pointing_elsewhere_is_followed() {
        let root = std::env::temp_dir().join(format!("myle-gitfile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("work")).unwrap();
        fs::create_dir_all(root.join("store")).unwrap();
        fs::write(root.join("store").join("index"), index_v2(&["a.txt"])).unwrap();
        fs::write(root.join("work").join(".git"), "gitdir: ../store\n").unwrap();
        assert_eq!(tracked_paths(&root.join("work")).unwrap(), ["a.txt"]);
        assert_eq!(tracked_paths(&root.join("store")), None);
        let _ = fs::remove_dir_all(root);
    }
}
