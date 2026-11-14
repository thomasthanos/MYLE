//! The registry half of a restore's safety copy. A game that keeps its saves
//! in the registry (`HKEY_CURRENT_USER\Software\…`) has every key the restore
//! writes read whole before it starts, values and subkeys, and put back the
//! same way when the restore does not finish cleanly or is undone.
use std::collections::BTreeMap;
use std::io;
use serde::{Deserialize, Serialize};
use winreg::enums::{DELETE, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, KEY_WRITE, RegType};
use winreg::{RegKey, RegValue};
/// A key as it was: its values, by name, and its subkeys.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct KeyCopy {
    pub values: BTreeMap<String, ValueCopy>,
    pub keys: BTreeMap<String, KeyCopy>,
}
/// A value's type and its bytes, as hex.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct ValueCopy {
    pub kind: u32,
    pub data: String,
}
/// The key `key` names (`HKEY_CURRENT_USER\Software\…`), read whole; `None`
/// when it does not exist.
pub(crate) fn capture(key: &str) -> io::Result<Option<KeyCopy>> {
    let (root, path) = split(key)?;
    match root.open_subkey_with_flags(path, KEY_READ) {
        Ok(opened) => read_tree(&opened).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
/// Makes `key` what `copy` holds: every value and subkey of the copy is
/// written, and what the copy does not have is removed. With no copy (the key
/// was not there), the key is removed. A failure part-way leaves the copy as
/// it is, so it can be put back again.
pub(crate) fn put_back(key: &str, copy: Option<&KeyCopy>) -> io::Result<()> {
    let (root, path) = split(key)?;
    let Some(copy) = copy else {
        return match root.delete_subkey_all(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    };
    let (opened, _) = root.create_subkey_with_flags(path, KEY_READ | KEY_WRITE | DELETE)?;
    write_tree(&opened, copy)
}
fn split(key: &str) -> io::Result<(RegKey, &str)> {
    let (root, path) = key.split_once('\\').unwrap_or((key, ""));
    let hive = match root.to_ascii_uppercase().as_str() {
        "HKEY_CURRENT_USER" | "HKCU" => HKEY_CURRENT_USER,
        "HKEY_LOCAL_MACHINE" | "HKLM" => HKEY_LOCAL_MACHINE,
        "HKEY_USERS" | "HKU" => HKEY_USERS,
        _ => return Err(io::Error::other(format!("{root} is not a part of the registry saves are kept in"))),
    };
    let path = path.trim_matches('\\');
    if path.is_empty() {
        return Err(io::Error::other("a whole part of the registry is never copied"));
    }
    Ok((RegKey::predef(hive), path))
}
fn read_tree(key: &RegKey) -> io::Result<KeyCopy> {
    let mut copy = KeyCopy::default();
    for value in key.enum_values() {
        let (name, value) = value?;
        copy.values.insert(name, ValueCopy { kind: value.vtype as u32, data: crate::download::to_hex(&value.bytes) });
    }
    for name in key.enum_keys() {
        let name = name?;
        let subkey = key.open_subkey_with_flags(&name, KEY_READ)?;
        copy.keys.insert(name, read_tree(&subkey)?);
    }
    Ok(copy)
}
fn write_tree(key: &RegKey, copy: &KeyCopy) -> io::Result<()> {
    for (name, value) in &copy.values {
        let bytes = from_hex(&value.data)?;
        key.set_raw_value(name, &RegValue { bytes: bytes.into(), vtype: reg_type(value.kind)? })?;
    }
    // Names in the registry ignore case.
    let present: Vec<String> = key.enum_values().map(|value| value.map(|(name, _)| name)).collect::<io::Result<_>>()?;
    for name in present.iter().filter(|name| !holds(copy.values.keys(), name)) {
        key.delete_value(name)?;
    }
    for (name, subkey) in &copy.keys {
        let (opened, _) = key.create_subkey_with_flags(name, KEY_READ | KEY_WRITE | DELETE)?;
        write_tree(&opened, subkey)?;
    }
    let present: Vec<String> = key.enum_keys().collect::<io::Result<_>>()?;
    for name in present.iter().filter(|name| !holds(copy.keys.keys(), name)) {
        key.delete_subkey_all(name)?;
    }
    Ok(())
}
fn holds<'a>(mut names: impl Iterator<Item = &'a String>, name: &str) -> bool {
    let name = name.to_lowercase();
    names.any(|kept| kept.to_lowercase() == name)
}
fn reg_type(kind: u32) -> io::Result<RegType> {
    use RegType::*;
    Ok(match kind {
        0 => REG_NONE,
        1 => REG_SZ,
        2 => REG_EXPAND_SZ,
        3 => REG_BINARY,
        4 => REG_DWORD,
        5 => REG_DWORD_BIG_ENDIAN,
        6 => REG_LINK,
        7 => REG_MULTI_SZ,
        8 => REG_RESOURCE_LIST,
        9 => REG_FULL_RESOURCE_DESCRIPTOR,
        10 => REG_RESOURCE_REQUIREMENTS_LIST,
        11 => REG_QWORD,
        other => return Err(io::Error::other(format!("unknown registry value type {other}"))),
    })
}
fn from_hex(text: &str) -> io::Result<Vec<u8>> {
    let bad = || io::Error::other("the copy of a registry value is damaged");
    if !text.len().is_multiple_of(2) {
        return Err(bad());
    }
    (0..text.len())
        .step_by(2)
        .map(|at| text.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()).ok_or_else(bad))
        .collect()
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// A key of its own under HKEY_CURRENT_USER\Software, removed when dropped.
    pub(crate) struct TestKey(pub String);
    impl TestKey {
        pub(crate) fn new(name: &str) -> Self {
            let key = Self(format!(r"HKEY_CURRENT_USER\Software\MYLE-test-{name}-{}", std::process::id()));
            put_back(&key.0, None).unwrap();
            key
        }
        pub(crate) fn open(&self) -> RegKey {
            let (root, path) = split(&self.0).unwrap();
            root.create_subkey_with_flags(path, KEY_READ | KEY_WRITE | DELETE).unwrap().0
        }
    }
    impl Drop for TestKey {
        fn drop(&mut self) {
            let _ = put_back(&self.0, None);
        }
    }
    #[test]
    fn a_key_is_put_back_exactly_values_and_subkeys() {
        let key = TestKey::new("put-back");
        let opened = key.open();
        opened.set_value("Slot", &"before").unwrap();
        opened.set_value("Gold", &120u32).unwrap();
        opened.set_raw_value("Blob", &RegValue { bytes: vec![0, 1, 254, 255].into(), vtype: RegType::REG_BINARY }).unwrap();
        opened.set_value("", &"default").unwrap();
        opened.create_subkey(r"Profiles\One").unwrap().0.set_value("Level", &7u64).unwrap();
        opened.create_subkey("Old").unwrap();
        let before = capture(&key.0).unwrap().unwrap();
        // The restore changes a value, adds one, adds a key and removes another.
        opened.set_value("Slot", &"after").unwrap();
        opened.set_value("New", &1u32).unwrap();
        opened.create_subkey(r"Profiles\Two").unwrap();
        opened.delete_subkey_all("Old").unwrap();
        put_back(&key.0, Some(&before)).unwrap();
        assert_eq!(capture(&key.0).unwrap().unwrap(), before);
        assert_eq!(opened.get_value::<String, _>("Slot").unwrap(), "before");
    }
    #[test]
    fn a_key_the_restore_made_is_removed_again() {
        let key = TestKey::new("made");
        put_back(&key.0, None).unwrap();
        assert_eq!(capture(&key.0).unwrap(), None);
        key.open().create_subkey("Inside").unwrap();
        put_back(&key.0, None).unwrap();
        assert_eq!(capture(&key.0).unwrap(), None);
        // Removing what is not there is not an error.
        put_back(&key.0, None).unwrap();
    }
    #[test]
    fn only_keys_inside_a_known_root_are_touched() {
        assert!(capture("HKEY_CLASSES_ROOT\\Something").is_err());
        assert!(capture("HKEY_CURRENT_USER").is_err());
        assert!(put_back("HKEY_CURRENT_USER\\", None).is_err());
    }
    #[test]
    fn hex_round_trips_and_damage_is_refused() {
        assert_eq!(from_hex(&crate::download::to_hex(&[0, 15, 16, 255])).unwrap(), vec![0, 15, 16, 255]);
        assert!(from_hex("abc").is_err());
        assert!(from_hex("zz").is_err());
    }
}
