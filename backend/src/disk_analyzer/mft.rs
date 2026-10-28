// ! The fast scan: the NTFS file table, read straight from the volume. ! ! On NTFS every file and folder has a record in the file table ($MFT, 1 KB ! a record, 4 KB on some disks), and the table is itself a file whose first ! record says where its pieces lie. Reading it in large sequential chunks is ! much quicker than listing every folder, and a record holds what the page ! needs: each name with the folder it is in (several for a hard link), the ! size, the size on disk, the attributes, the reparse tag and the date. ! Opening a volume needs an administrator, so the elevated helper ! (`elevated.rs`) reads the table and sends the app a compact snapshot, and ! the app builds from it the same tree the walk builds. ! ! The layouts are the publicly documented NTFS ones: the boot sector, FILE ! records and their update sequence (the last two bytes of each 512-byte ! block are kept apart, so a torn write shows), attributes and data runs. ! The volume is only read. ! ! Rules: a hard link counts once, where it is met first walking down from ! the root (System32 before WinSxS); its other names are listed with no size ! and marked. The file system's own files ($MFT, $LogFile…) are listed at ! the root like other system files, so the total is close to the space ! used. Size is the main data stream's; size on disk adds the clusters every ! stream and a folder's index really have, from their data runs (a hole in ! a sparse or compressed stream takes nothing). A name that could not come ! from Windows (a path separator, `..`) is dropped, on both sides.
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use super::scan::{PROGRESS_EVERY, Progress, Throttle, kind_of, push_name};
use super::tree::{Entry, Kind, Listing, Tree, unix_seconds};
/// The root folder's record.
pub const ROOT: u32 = 5;
/// The table is read this much at a time.
const CHUNK: usize = 4 << 20;
/// The update sequence protects blocks of this size.
const BLOCK: usize = 512;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
// Attribute types.
const STANDARD_INFORMATION: u32 = 0x10;
const ATTRIBUTE_LIST: u32 = 0x20;
const FILE_NAME: u32 = 0x30;
const DATA: u32 = 0x80;
const INDEX_ALLOCATION: u32 = 0xA0;
const REPARSE_POINT: u32 = 0xC0;
const END: u32 = 0xFFFF_FFFF;
const RECORD_IN_USE: u16 = 0x1;
const RECORD_DIRECTORY: u16 = 0x2;
/// The short 8.3 names, kept beside the real ones.
const DOS_NAMESPACE: u8 = 2;
/// A file reference's record number; the top 16 bits are its sequence number.
const NUMBER_MASK: u64 = 0xFFFF_FFFF_FFFF;
pub const NOT_NTFS: &str = "This drive is not NTFS.";
pub const BAD_TABLE: &str = "The drive's file table could not be read.";
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    bytes.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    bytes.get(at..at + 8).map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap_or_default()))
}
// --------------------------------------------------------------------------- The boot sector, records and data runs: pure functions
/// What the boot sector says about the volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// Bytes per sector: reads start and end on these.
    pub sector: u64,
    /// Bytes per cluster.
    pub cluster: u64,
    /// Bytes per file record.
    pub record: u64,
    /// The table's first cluster.
    pub mft_lcn: u64,
}
pub fn geometry(boot: &[u8]) -> Result<Geometry, String> {
    if boot.len() < BLOCK || &boot[3..11] != b"NTFS    " {
        return Err(NOT_NTFS.into());
    }
    let sector = u16_at(boot, 0x0B) as u64;
    // Above 0x80 it is a power of two: 0xF4 means 2^(256 - 0xF4) sectors.
    let raw = boot[0x0D];
    let per_cluster = if raw > 0x80 { 1u64.checked_shl(256 - raw as u32).unwrap_or(0) } else { raw as u64 };
    let cluster = sector * per_cluster;
    // Negative: 2^-n bytes; else n clusters.
    let per_record = boot[0x40] as i8;
    let record = if per_record < 0 {
        1u64.checked_shl(per_record.unsigned_abs() as u32).unwrap_or(0)
    } else {
        per_record as u64 * cluster
    };
    let mft_lcn = u64_at(boot, 0x30);
    let sane = matches!(sector, 512 | 1024 | 2048 | 4096)
        && cluster.is_power_of_two()
        && (sector..=2 << 20).contains(&cluster)
        && matches!(record, 1024 | 2048 | 4096)
        && mft_lcn > 0;
    if !sane {
        return Err(BAD_TABLE.into());
    }
    Ok(Geometry { sector, cluster, record, mft_lcn })
}
/// Puts back the last two bytes of each 512-byte block from the update
/// sequence. `false` when the record is not whole (a torn write) or is not
/// a record at all.
pub fn fixup(record: &mut [u8]) -> bool {
    let offset = u16_at(record, 4) as usize;
    let count = u16_at(record, 6) as usize;
    if count < 2 || (count - 1) * BLOCK != record.len() || offset + count * 2 > BLOCK {
        return false;
    }
    let check = [record[offset], record[offset + 1]];
    for block in 1..count {
        let end = block * BLOCK;
        if record[end - 2..end] != check {
            return false;
        }
        record[end - 2] = record[offset + block * 2];
        record[end - 1] = record[offset + block * 2 + 1];
    }
    true
}
/// A piece of a stream on the disk: `clusters` from cluster `lcn`, or a
/// hole of a sparse stream (`None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub lcn: Option<u64>,
    pub clusters: u64,
}
/// Decodes data runs: each starts with a byte whose low half is how many
/// bytes the length takes and whose high half how many the offset from the
/// last run takes (none for a hole). `None` when they do not make sense.
pub fn runs(bytes: &[u8]) -> Option<Vec<Run>> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut lcn: i64 = 0;
    while let Some(&head) = bytes.get(at) {
        if head == 0 {
            return Some(out);
        }
        let (length_size, offset_size) = ((head & 0xF) as usize, (head >> 4) as usize);
        if length_size == 0 || length_size > 8 || offset_size > 8 {
            return None;
        }
        let length = bytes.get(at + 1..at + 1 + length_size)?;
        let offset = bytes.get(at + 1 + length_size..at + 1 + length_size + offset_size)?;
        let clusters = unsigned(length);
        if offset_size == 0 {
            out.push(Run { lcn: None, clusters });
        } else {
            lcn = lcn.checked_add(signed(offset))?;
            out.push(Run { lcn: Some(u64::try_from(lcn).ok()?), clusters });
        }
        at += 1 + length_size + offset_size;
    }
    Some(out)
}
fn unsigned(bytes: &[u8]) -> u64 {
    bytes.iter().rev().fold(0, |value, &b| (value << 8) | b as u64)
}
fn signed(bytes: &[u8]) -> i64 {
    let value = unsigned(bytes);
    let bits = bytes.len() * 8;
    if bits < 64 && value & (1 << (bits - 1)) != 0 { (value | (!0u64 << bits)) as i64 } else { value as i64 }
}
/// What one FILE record says. Reused from record to record.
#[derive(Default)]
pub struct Record {
    pub in_use: bool,
    pub dir: bool,
    pub seq: u16,
    /// For an extension record: the base record it adds to.
    pub base: Option<u32>,
    /// From $STANDARD_INFORMATION, which only a base record has.
    pub attributes: Option<u32>,
    pub modified: Option<i64>,
    pub tag: Option<u32>,
    /// Each name: its folder's reference and where its UTF-16 is in `units`.
    pub names: Vec<(u64, usize, usize)>,
    pub units: Vec<u16>,
    /// The main data stream's size, from its first piece.
    pub size: Option<u64>,
    /// What the streams and a folder's index whose first piece is here take.
    pub alloc: u64,
    /// The main stream's runs by first cluster, and the records with more of
    /// them: kept for the table's own record only.
    pub pieces: Vec<(u64, Vec<Run>)>,
    pub parts: Vec<u32>,
}
impl Record {
    fn clear(&mut self) {
        let (mut names, mut units, mut pieces, mut parts) =
            (std::mem::take(&mut self.names), std::mem::take(&mut self.units), std::mem::take(&mut self.pieces), std::mem::take(&mut self.parts));
        names.clear();
        units.clear();
        pieces.clear();
        parts.clear();
        *self = Record { names, units, pieces, parts, ..Record::default() };
    }
}
/// Reads one FILE record whose update sequence was put back, on a volume of
/// `cluster`-byte clusters. `whole`: keep the main stream's runs and
/// attribute list too (for the table's own record). `false` when it is not a
/// record.
pub fn parse(bytes: &[u8], out: &mut Record, whole: bool, cluster: u64) -> bool {
    out.clear();
    if bytes.len() < 0x30 || &bytes[..4] != b"FILE" {
        return false;
    }
    let flags = u16_at(bytes, 0x16);
    out.in_use = flags & RECORD_IN_USE != 0;
    if !out.in_use {
        return true;
    }
    out.dir = flags & RECORD_DIRECTORY != 0;
    out.seq = u16_at(bytes, 0x10);
    let base = u64_at(bytes, 0x20) & NUMBER_MASK;
    if base != 0 {
        let Ok(base) = u32::try_from(base) else { return false };
        out.base = Some(base);
    }
    let used = (u32_at(bytes, 0x18) as usize).min(bytes.len());
    let mut at = u16_at(bytes, 0x14) as usize;
    while at + 16 <= used {
        let kind = u32_at(bytes, at);
        let length = u32_at(bytes, at + 4) as usize;
        if kind == END || length < 16 || at + length > used {
            break;
        }
        let attribute = &bytes[at..at + length];
        at += length;
        let named = attribute[9] != 0;
        if attribute[8] == 0 {
            // Resident: the value is in the record.
            let value_at = u16_at(attribute, 0x14) as usize;
            let value_length = u32_at(attribute, 0x10) as usize;
            let Some(value) = attribute.get(value_at..value_at.saturating_add(value_length)) else { continue };
            match kind {
                STANDARD_INFORMATION if value.len() >= 0x24 => {
                    out.modified = Some(u64_at(value, 0x08) as i64);
                    out.attributes = Some(u32_at(value, 0x20));
                }
                FILE_NAME if value.len() >= 0x42 => {
                    let count = value[0x40] as usize;
                    let Some(name) = value.get(0x42..0x42 + count * 2) else { continue };
                    if value[0x41] != DOS_NAMESPACE {
                        let start = out.units.len();
                        out.units.extend(name.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)));
                        out.names.push((u64_at(value, 0), start, out.units.len()));
                    }
                }
                DATA if !named => out.size = Some(value.len() as u64),
                REPARSE_POINT => out.tag = Some(u32_at(value, 0)),
                ATTRIBUTE_LIST if whole => list_parts(value, &mut out.parts),
                _ => {}
            }
        } else {
            if attribute.len() < 0x40 {
                continue;
            }
            let first_vcn = u64_at(attribute, 0x10);
            let runs_at = u16_at(attribute, 0x20) as usize;
            if matches!(kind, DATA | INDEX_ALLOCATION) {
                // What a stream takes is the clusters it really has: the header's allocated size counts holes too, and the bad cluster file's stream "allocates" the whole volume that way.
                let found = attribute.get(runs_at..).and_then(runs);
                let clusters: u64 = found.iter().flatten().filter(|run| run.lcn.is_some()).map(|run| run.clusters).sum();
                out.alloc += clusters * cluster;
                // The size is only in a stream's first piece.
                if kind == DATA && !named && first_vcn == 0 {
                    out.size = Some(u64_at(attribute, 0x30));
                }
                if kind == DATA && whole && !named
                    && let Some(found) = found
                {
                    out.pieces.push((first_vcn, found));
                }
            } else if kind == REPARSE_POINT {
                // Reparse data too big for the record: what kind is not known.
                out.tag = Some(0);
            }
        }
    }
    true
}
/// A name Windows could have given: not empty, not `.` or `..`, and without
/// a path separator, a stream's colon or another character Windows refuses.
/// The table is read raw, so a damaged record must not name another path:
/// a name joined to its folder's path is what goes to the Recycle Bin.
pub fn plain(name: &[u8]) -> bool {
    !name.is_empty() && name != b"." && name != b".." && !name.iter().any(|&b| b < 0x20 || br#"\/:*?"<>|"#.contains(&b))
}
/// The records an attribute list puts the main data stream's pieces in.
fn list_parts(list: &[u8], parts: &mut Vec<u32>) {
    let mut at = 0;
    while at + 0x1A <= list.len() {
        let length = u16_at(list, at + 4) as usize;
        if length < 0x1A {
            return;
        }
        if u32_at(list, at) == DATA && list[at + 6] == 0
            && let Ok(number) = u32::try_from(u64_at(list, at + 0x10) & NUMBER_MASK)
            && !parts.contains(&number)
        {
            parts.push(number);
        }
        at += length;
    }
}
// --------------------------------------------------------------------------- The snapshot: what the helper sends the app
/// A file or folder of the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileInfo {
    /// Its record number.
    pub index: u32,
    pub dir: bool,
    pub reparse: bool,
    pub attributes: u32,
    pub tag: u32,
    /// Seconds since 1970.
    pub modified: u32,
    pub size: u64,
    pub alloc: u64,
}
/// One name of a file, in a folder. The names follow each other in `names`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    /// The file's place in `files`.
    pub file: u32,
    /// The folder's record number.
    pub parent: u32,
    pub name_len: u16,
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// By record number.
    pub files: Vec<FileInfo>,
    pub links: Vec<Link>,
    /// WTF-8, as `OsStr::as_encoded_bytes`.
    pub names: Vec<u8>,
}
const MAGIC: &[u8; 8] = b"MYLEMFT1";
const FILE_BYTES: usize = 33;
const LINK_BYTES: usize = 10;
impl Snapshot {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(24 + self.files.len() * FILE_BYTES + self.links.len() * LINK_BYTES + self.names.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.links.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.names.len() as u64).to_le_bytes());
        for file in &self.files {
            out.extend_from_slice(&file.index.to_le_bytes());
            out.push(u8::from(file.dir) | u8::from(file.reparse) << 1);
            out.extend_from_slice(&file.attributes.to_le_bytes());
            out.extend_from_slice(&file.tag.to_le_bytes());
            out.extend_from_slice(&file.modified.to_le_bytes());
            out.extend_from_slice(&file.size.to_le_bytes());
            out.extend_from_slice(&file.alloc.to_le_bytes());
        }
        for link in &self.links {
            out.extend_from_slice(&link.file.to_le_bytes());
            out.extend_from_slice(&link.parent.to_le_bytes());
            out.extend_from_slice(&link.name_len.to_le_bytes());
        }
        out.extend_from_slice(&self.names);
        out
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let bad = || BAD_TABLE.to_string();
        if bytes.len() < 24 || &bytes[..8] != MAGIC {
            return Err(bad());
        }
        let files = u32_at(bytes, 8) as usize;
        let links = u32_at(bytes, 12) as usize;
        let names = usize::try_from(u64_at(bytes, 16)).map_err(|_| bad())?;
        let expected = files
            .checked_mul(FILE_BYTES)
            .and_then(|f| links.checked_mul(LINK_BYTES).and_then(|l| f.checked_add(l)))
            .and_then(|n| n.checked_add(24 + names))
            .ok_or_else(bad)?;
        if expected != bytes.len() {
            return Err(bad());
        }
        let mut at = 24;
        let mut out = Snapshot { files: Vec::with_capacity(files), links: Vec::with_capacity(links), names: Vec::new() };
        for _ in 0..files {
            let b = &bytes[at..at + FILE_BYTES];
            out.files.push(FileInfo {
                index: u32_at(b, 0),
                dir: b[4] & 1 != 0,
                reparse: b[4] & 2 != 0,
                attributes: u32_at(b, 5),
                tag: u32_at(b, 9),
                modified: u32_at(b, 13),
                size: u64_at(b, 17),
                alloc: u64_at(b, 25),
            });
            at += FILE_BYTES;
        }
        let mut named = 0usize;
        for _ in 0..links {
            let b = &bytes[at..at + LINK_BYTES];
            let link = Link { file: u32_at(b, 0), parent: u32_at(b, 4), name_len: u16_at(b, 8) };
            if link.file as usize >= files {
                return Err(bad());
            }
            named += link.name_len as usize;
            out.links.push(link);
            at += LINK_BYTES;
        }
        if named != names || !out.files.windows(2).all(|w| w[0].index < w[1].index) {
            return Err(bad());
        }
        out.names = bytes[at..].to_vec();
        Ok(out)
    }
}
/// Gathers records as the table is read, by record number: an extension
/// record adds to its base record, whichever comes first.
pub struct Table {
    slots: Vec<Slot>,
    links: Vec<RawLink>,
    names: Vec<u8>,
}
const IN_USE: u8 = 1;
const DIR: u8 = 2;
const REPARSE: u8 = 4;
#[derive(Clone, Copy, Default)]
struct Slot {
    flags: u8,
    seq: u16,
    attributes: u32,
    tag: u32,
    modified: u32,
    size: u64,
    alloc: u64,
}
struct RawLink {
    record: u32,
    parent: u64,
    name_len: u16,
}
impl Table {
    pub fn new(records: usize) -> Self {
        Self { slots: vec![Slot::default(); records], links: Vec::new(), names: Vec::new() }
    }
    /// Adds a record in use.
    pub fn add(&mut self, number: u32, record: &Record) {
        let target = record.base.unwrap_or(number);
        let Some(slot) = self.slots.get_mut(target as usize) else { return };
        if record.base.is_none() {
            slot.flags |= IN_USE | if record.dir { DIR } else { 0 };
            slot.seq = record.seq;
            slot.attributes = record.attributes.unwrap_or(slot.attributes);
            slot.modified = record.modified.map_or(slot.modified, unix_seconds);
        }
        if let Some(size) = record.size {
            slot.size = size;
        }
        slot.alloc += record.alloc;
        if let Some(tag) = record.tag {
            slot.tag = tag;
            slot.flags |= REPARSE;
        }
        for &(parent, start, end) in &record.names {
            let before = self.names.len();
            push_name(&mut self.names, &record.units[start..end]);
            if !plain(&self.names[before..]) {
                self.names.truncate(before);
                continue;
            }
            let name_len = u16::try_from(self.names.len() - before).unwrap_or(u16::MAX);
            self.links.push(RawLink { record: target, parent, name_len });
        }
    }
    /// The files in use, and the names whose folder is in use, is a folder,
    /// and has the sequence number the name expects (else the name is left
    /// from a folder deleted since).
    pub fn finish(self) -> Snapshot {
        let Table { slots, links, names } = self;
        let mut place = vec![u32::MAX; slots.len()];
        let mut files = Vec::new();
        for (index, slot) in slots.iter().enumerate() {
            if slot.flags & IN_USE != 0 {
                place[index] = files.len() as u32;
                files.push(FileInfo {
                    index: index as u32,
                    dir: slot.flags & DIR != 0,
                    reparse: slot.flags & REPARSE != 0,
                    attributes: slot.attributes,
                    tag: slot.tag,
                    modified: slot.modified,
                    size: slot.size,
                    alloc: slot.alloc,
                });
            }
        }
        let mut kept = Snapshot { files, links: Vec::with_capacity(links.len()), names: Vec::with_capacity(names.len()) };
        let mut at = 0usize;
        for link in links {
            let name = &names[at..at + link.name_len as usize];
            at += link.name_len as usize;
            let parent = (link.parent & NUMBER_MASK) as usize;
            let seq = (link.parent >> 48) as u16;
            let file = place[link.record as usize];
            let Some(folder) = slots.get(parent) else { continue };
            if file == u32::MAX
                || parent == link.record as usize
                || folder.flags & (IN_USE | DIR) != IN_USE | DIR
                || (seq != 0 && seq != folder.seq)
            {
                continue;
            }
            kept.names.extend_from_slice(name);
            kept.links.push(Link { file, parent: parent as u32, name_len: link.name_len });
        }
        kept
    }
}
// --------------------------------------------------------------------------- Reading a volume
/// Reads `buffer.len()` bytes at `offset`; both on sector boundaries.
pub trait ReadAt {
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()>;
}
impl ReadAt for std::fs::File {
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buffer.len() {
            match self.seek_read(&mut buffer[done..], offset + done as u64)? {
                0 => return Err(io::ErrorKind::UnexpectedEof.into()),
                n => done += n,
            }
        }
        Ok(())
    }
}
impl ReadAt for [u8] {
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        let source = self.get(start..start + buffer.len()).ok_or(io::ErrorKind::UnexpectedEof)?;
        buffer.copy_from_slice(source);
        Ok(())
    }
}
fn read_error(error: io::Error) -> String {
    format!("{BAD_TABLE} {error}")
}
/// One record at `offset`, its update sequence put back.
fn read_record(volume: &(impl ReadAt + ?Sized), geometry: &Geometry, offset: u64) -> Result<Vec<u8>, String> {
    let start = offset / geometry.sector * geometry.sector;
    let end = (offset + geometry.record).div_ceil(geometry.sector) * geometry.sector;
    let mut buffer = vec![0u8; (end - start) as usize];
    volume.read_at(&mut buffer, start).map_err(read_error)?;
    let at = (offset - start) as usize;
    let mut bytes = buffer[at..at + geometry.record as usize].to_vec();
    if !fixup(&mut bytes) {
        return Err(BAD_TABLE.into());
    }
    Ok(bytes)
}
/// Where on the disk record `number` is, from the table's pieces known so far.
fn locate(pieces: &[(u64, Vec<Run>)], geometry: &Geometry, number: u32) -> Option<u64> {
    let byte = number as u64 * geometry.record;
    let vcn = byte / geometry.cluster;
    for (first, runs) in pieces {
        let mut at = *first;
        for run in runs {
            if (at..at + run.clusters).contains(&vcn) {
                return run.lcn.map(|lcn| (lcn + vcn - at) * geometry.cluster + byte % geometry.cluster);
            }
            at += run.clusters;
        }
    }
    None
}
/// Reads a whole file table from `volume` (a volume, or an image of one).
pub fn read_table(volume: &(impl ReadAt + ?Sized)) -> Result<Snapshot, String> {
    let mut boot = vec![0u8; 4096];
    volume.read_at(&mut boot, 0).map_err(read_error)?;
    let geometry = geometry(&boot)?;
    let record_len = geometry.record as usize;
    let mut record = Record::default();
    // The table's own record says where its pieces are; a very fragmented table lists the rest of them in extension records.
    let first = read_record(volume, &geometry, geometry.mft_lcn * geometry.cluster)?;
    if !parse(&first, &mut record, true, geometry.cluster) || !record.in_use || record.base.is_some() {
        return Err(BAD_TABLE.into());
    }
    let mut size = record.size;
    let mut pieces = std::mem::take(&mut record.pieces);
    for number in std::mem::take(&mut record.parts) {
        if number == 0 {
            continue;
        }
        let offset = locate(&pieces, &geometry, number).ok_or(BAD_TABLE)?;
        let bytes = read_record(volume, &geometry, offset)?;
        if parse(&bytes, &mut record, true, geometry.cluster) && record.in_use && record.base == Some(0) {
            pieces.append(&mut record.pieces);
            size = size.or(record.size);
        }
    }
    let size = size.ok_or(BAD_TABLE)?;
    pieces.sort_by_key(|(vcn, _)| *vcn);
    let mut extents: Vec<(Option<u64>, u64)> = Vec::new();
    let mut vcn = 0u64;
    for (first, runs) in &pieces {
        if *first != vcn {
            return Err(BAD_TABLE.into());
        }
        for run in runs {
            extents.push((run.lcn.map(|lcn| lcn * geometry.cluster), run.clusters * geometry.cluster));
            vcn += run.clusters;
        }
    }
    let count = size / geometry.record;
    if vcn * geometry.cluster < size || count > u32::MAX as u64 {
        return Err(BAD_TABLE.into());
    }
    let mut table = Table::new(count as usize);
    let mut pending: Vec<u8> = Vec::with_capacity(CHUNK + record_len);
    let mut buffer = vec![0u8; CHUNK];
    let mut number = 0u64;
    'read: for (disk, length) in extents {
        let mut done = 0u64;
        while done < length {
            if number >= count {
                break 'read;
            }
            let take = (length - done).min(CHUNK as u64) as usize;
            match disk {
                Some(at) => volume.read_at(&mut buffer[..take], at + done).map_err(read_error)?,
                None => buffer[..take].fill(0),
            }
            pending.extend_from_slice(&buffer[..take]);
            let mut used = 0;
            while pending.len() - used >= record_len && number < count {
                let bytes = &mut pending[used..used + record_len];
                if fixup(bytes) && parse(bytes, &mut record, false, geometry.cluster) && record.in_use {
                    table.add(number as u32, &record);
                }
                used += record_len;
                number += 1;
            }
            pending.drain(..used);
            done += take as u64;
        }
    }
    Ok(table.finish())
}
/// Reads the file table of fixed NTFS drive `letter`. Needs an administrator.
pub fn read_volume(letter: char) -> Result<Snapshot, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW};
    const FIXED: u32 = 3;
    let letter = letter.to_ascii_uppercase();
    if !letter.is_ascii_uppercase() {
        return Err("That is not a drive.".into());
    }
    let root: Vec<u16> = format!(r"{letter}:\").encode_utf16().chain([0]).collect();
    // SAFETY: a NUL-terminated string that outlives the call.
    if unsafe { GetDriveTypeW(root.as_ptr()) } != FIXED {
        return Err("Only a fixed drive's file table is read.".into());
    }
    let volume = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(format!(r"\\.\{letter}:"))
        .map_err(|e| format!("{letter}: could not be opened for reading: {e}"))?;
    read_table(&volume)
}
// --------------------------------------------------------------------------- The app's side: the tree
/// Builds the tree the walk builds, from the root folder down, for the
/// volume whose root Windows shows as `root`. `None` when cancelled.
pub fn into_tree(
    snapshot: &Snapshot,
    scan_id: u64,
    root: PathBuf,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(&Progress),
) -> Result<Option<Tree>, String> {
    let Snapshot { files, links, names } = snapshot;
    let root_at = files.binary_search_by_key(&ROOT, |f| f.index).map_err(|_| BAD_TABLE.to_string())?;
    let mut name_at = Vec::with_capacity(links.len());
    let mut at = 0usize;
    for link in links {
        name_at.push(at);
        at += link.name_len as usize;
    }
    // Each folder's names together: a counting sort by folder.
    let folders = files.last().map_or(0, |f| f.index as usize + 1);
    let mut start = vec![0u32; folders + 1];
    for link in links.iter().filter(|l| (l.parent as usize) < folders) {
        start[link.parent as usize + 1] += 1;
    }
    for i in 1..start.len() {
        start[i] += start[i - 1];
    }
    let mut next = start.clone();
    let mut grouped = vec![0u32; links.len()];
    for (i, link) in links.iter().enumerate().filter(|(_, l)| (l.parent as usize) < folders) {
        grouped[next[link.parent as usize] as usize] = i as u32;
        next[link.parent as usize] += 1;
    }
    let mut tree = Tree::new(scan_id, root, files[root_at].modified);
    let mut counted = vec![false; files.len()];
    let mut opened = vec![false; files.len()];
    opened[root_at] = true;
    let mut queue = VecDeque::from([(0u32, ROOT)]);
    let mut progress = Progress::default();
    let mut throttle = Throttle::new(PROGRESS_EVERY, Instant::now);
    let mut listing = Listing::default();
    let (mut hard_links, mut below) = (Vec::new(), Vec::new());
    while let Some((id, folder)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        listing.names.clear();
        listing.entries.clear();
        hard_links.clear();
        below.clear();
        let group = &grouped[start[folder as usize] as usize..start[folder as usize + 1] as usize];
        for &l in group {
            let link = links[l as usize];
            let name = &names[name_at[l as usize]..name_at[l as usize] + link.name_len as usize];
            // The snapshot crossed from another process: checked again.
            if !plain(name) {
                continue;
            }
            let n = listing.entries.len();
            let file = &files[link.file as usize];
            let attributes = file.attributes
                | if file.dir { FILE_ATTRIBUTE_DIRECTORY } else { 0 }
                | if file.reparse { FILE_ATTRIBUTE_REPARSE_POINT } else { 0 };
            let kind = kind_of(attributes, file.tag);
            let (size, alloc) = if file.dir {
                (0, file.alloc)
            } else if std::mem::replace(&mut counted[link.file as usize], true) {
                hard_links.push(n as u32);
                (0, 0)
            } else {
                (file.size, file.alloc)
            };
            match kind {
                Kind::Dir => {
                    progress.dirs += 1;
                    if !std::mem::replace(&mut opened[link.file as usize], true) {
                        below.push((n as u32, file.index));
                    }
                }
                Kind::File | Kind::Cloud => {
                    progress.files += 1;
                    progress.bytes += size;
                    progress.alloc += alloc;
                }
                Kind::Link => {}
            }
            listing.names.extend_from_slice(name);
            listing.entries.push(Entry {
                name_len: name.len() as u32,
                kind,
                size,
                alloc,
                modified: file.modified,
                system: file.attributes & FILE_ATTRIBUTE_SYSTEM != 0,
            });
        }
        let ids = tree.add_children(id, &listing);
        for &n in &hard_links {
            tree.mark_hard_link(ids.start + n);
        }
        for &(n, folder) in &below {
            queue.push_back((ids.start + n, folder));
        }
        if throttle.ready() {
            on_progress(&progress);
        }
    }
    tree.finish();
    Ok(Some(tree))
}
#[cfg(test)]
mod tests {
    use super::super::tree::{SizeMode, flag};
    use super::*;
    const RECORD: usize = 1024;
    const CLUSTER: u64 = 4096;
    fn refer(number: u64, seq: u16) -> u64 {
        number | (seq as u64) << 48
    }
    fn pad8(mut bytes: Vec<u8>) -> Vec<u8> {
        bytes.resize(bytes.len().div_ceil(8) * 8, 0);
        bytes
    }
    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }
    fn resident(kind: u32, name: &str, value: &[u8]) -> Vec<u8> {
        let name = utf16(name);
        let value_at = (0x18 + name.len()).div_ceil(8) * 8;
        let mut a = vec![0u8; value_at];
        a[0..4].copy_from_slice(&kind.to_le_bytes());
        a[9] = (name.len() / 2) as u8;
        a[0x0A..0x0C].copy_from_slice(&0x18u16.to_le_bytes());
        a[0x10..0x14].copy_from_slice(&(value.len() as u32).to_le_bytes());
        a[0x14..0x16].copy_from_slice(&(value_at as u16).to_le_bytes());
        a[0x18..0x18 + name.len()].copy_from_slice(&name);
        a.extend_from_slice(value);
        let mut a = pad8(a);
        let length = a.len() as u32;
        a[4..8].copy_from_slice(&length.to_le_bytes());
        a
    }
    /// A non-resident stream's first piece; `packed`: compressed or sparse,
    /// with what it really takes.
    fn non_resident(kind: u32, name: &str, real: u64, allocated: u64, packed: Option<u64>, runs: &[u8]) -> Vec<u8> {
        let name = utf16(name);
        let header = if packed.is_some() { 0x48 } else { 0x40 };
        let name_at = header;
        let runs_at = (name_at + name.len()).div_ceil(8) * 8;
        let mut a = vec![0u8; runs_at];
        a[0..4].copy_from_slice(&kind.to_le_bytes());
        a[8] = 1;
        a[9] = (name.len() / 2) as u8;
        a[0x0A..0x0C].copy_from_slice(&(name_at as u16).to_le_bytes());
        if packed.is_some() {
            a[0x0C..0x0E].copy_from_slice(&0x8000u16.to_le_bytes());
        }
        a[0x20..0x22].copy_from_slice(&(runs_at as u16).to_le_bytes());
        a[0x28..0x30].copy_from_slice(&allocated.to_le_bytes());
        a[0x30..0x38].copy_from_slice(&real.to_le_bytes());
        a[0x38..0x40].copy_from_slice(&real.to_le_bytes());
        if let Some(on_disk) = packed {
            a[0x40..0x48].copy_from_slice(&on_disk.to_le_bytes());
        }
        a[name_at..name_at + name.len()].copy_from_slice(&name);
        a.extend_from_slice(runs);
        a.push(0);
        let mut a = pad8(a);
        let length = a.len() as u32;
        a[4..8].copy_from_slice(&length.to_le_bytes());
        a
    }
    fn standard(attributes: u32) -> Vec<u8> {
        let mut v = vec![0u8; 0x48];
        // 2023-11-14 22:13:20 UTC.
        let filetime: u64 = (1_700_000_000 + 11_644_473_600) * 10_000_000;
        v[0x08..0x10].copy_from_slice(&filetime.to_le_bytes());
        v[0x20..0x24].copy_from_slice(&attributes.to_le_bytes());
        resident(STANDARD_INFORMATION, "", &v)
    }
    fn name(parent: u64, text: &str, namespace: u8) -> Vec<u8> {
        let units = utf16(text);
        let mut v = vec![0u8; 0x42];
        v[0..8].copy_from_slice(&parent.to_le_bytes());
        v[0x40] = (units.len() / 2) as u8;
        v[0x41] = namespace;
        v.extend_from_slice(&units);
        resident(FILE_NAME, "", &v)
    }
    /// A whole record with its update sequence applied, as on the disk.
    fn record(flags: u16, seq: u16, base: u64, attributes: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; RECORD];
        r[..4].copy_from_slice(b"FILE");
        let count = RECORD / BLOCK + 1;
        r[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        r[6..8].copy_from_slice(&(count as u16).to_le_bytes());
        r[0x10..0x12].copy_from_slice(&seq.to_le_bytes());
        let first = (0x30 + count * 2).div_ceil(8) * 8;
        r[0x14..0x16].copy_from_slice(&(first as u16).to_le_bytes());
        r[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        r[0x1C..0x20].copy_from_slice(&(RECORD as u32).to_le_bytes());
        r[0x20..0x28].copy_from_slice(&base.to_le_bytes());
        let mut at = first;
        for attribute in attributes {
            r[at..at + attribute.len()].copy_from_slice(attribute);
            at += attribute.len();
        }
        r[at..at + 4].copy_from_slice(&END.to_le_bytes());
        r[0x18..0x1C].copy_from_slice(&((at + 8) as u32).to_le_bytes());
        let check = [0x34, 0x12];
        r[0x30..0x32].copy_from_slice(&check);
        for block in 1..count {
            let end = block * BLOCK;
            let (head, tail) = r.split_at_mut(end - 2);
            head[0x30 + block * 2..0x30 + block * 2 + 2].copy_from_slice(&tail[..2]);
            tail[..2].copy_from_slice(&check);
        }
        r
    }
    const FILE: u16 = RECORD_IN_USE;
    const FOLDER: u16 = RECORD_IN_USE | RECORD_DIRECTORY;
    fn boot(mft_lcn: u64) -> Vec<u8> {
        let mut b = vec![0u8; 4096];
        b[3..11].copy_from_slice(b"NTFS    ");
        b[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        b[0x0D] = 8;
        b[0x30..0x38].copy_from_slice(&mft_lcn.to_le_bytes());
        b[0x40] = (-10i8) as u8;
        b
    }
    #[test]
    fn the_boot_sector_gives_the_volumes_geometry() {
        assert_eq!(geometry(&boot(786_432)).unwrap(), Geometry { sector: 512, cluster: 4096, record: 1024, mft_lcn: 786_432 });
        let mut big = boot(4);
        // 2^(256 - 0xF8) = 256 sectors a cluster: 128 KB clusters.
        big[0x0D] = 0xF8;
        assert_eq!(geometry(&big).unwrap().cluster, 128 * 1024);
        let mut other = boot(4);
        other[3..11].copy_from_slice(b"EXFAT   ");
        assert_eq!(geometry(&other).unwrap_err(), NOT_NTFS);
        let mut odd = boot(4);
        odd[0x40] = 3;
        assert_eq!(geometry(&odd).unwrap_err(), BAD_TABLE, "a 12 KB record makes no sense");
    }
    #[test]
    fn the_update_sequence_is_put_back_and_a_torn_record_is_refused() {
        let original = record(FILE, 1, 0, &[standard(0), name(refer(5, 5), "a.txt", 1)]);
        let mut bytes = original.clone();
        assert_eq!(&bytes[510..512], &[0x34, 0x12], "on the disk, the block ends hold the check");
        assert!(fixup(&mut bytes));
        assert_ne!(&bytes[510..512], &[0x34, 0x12]);
        let mut torn = original.clone();
        torn[1022] ^= 0xFF;
        assert!(!fixup(&mut torn));
        assert!(!fixup(&mut vec![0u8; RECORD]), "an empty slot");
    }
    #[test]
    fn data_runs_are_decoded() {
        // 0x18 clusters at 0x5634, 0x08 clusters 0x100 back, a hole, the end.
        let bytes = [0x21, 0x18, 0x34, 0x56, 0x21, 0x08, 0x00, 0xFF, 0x01, 0x10, 0x00];
        assert_eq!(
            runs(&bytes).unwrap(),
            [
                Run { lcn: Some(0x5634), clusters: 0x18 },
                Run { lcn: Some(0x5634 - 0x100), clusters: 0x08 },
                Run { lcn: None, clusters: 0x10 },
            ]
        );
        assert!(runs(&[0x21, 0x18]).is_none(), "cut short");
        assert!(runs(&[0x11, 0x01, 0x80]).is_none(), "before the first cluster");
    }
    #[test]
    fn a_record_gives_names_sizes_and_kind() {
        let mut bytes = record(
            FILE,
            3,
            0,
            &[
                standard(0x20),
                name(refer(16, 1), "Long File Name.txt", 1),
                name(refer(16, 1), "LONGFI~1.TXT", DOS_NAMESPACE),
                resident(DATA, "", b"hello world"),
                resident(DATA, "Zone.Identifier", b"[ZoneTransfer]"),
            ],
        );
        assert!(fixup(&mut bytes));
        let mut r = Record::default();
        assert!(parse(&bytes, &mut r, false, CLUSTER));
        assert!(r.in_use && !r.dir);
        assert_eq!(r.seq, 3);
        assert_eq!(r.attributes, Some(0x20));
        assert_eq!(r.size, Some(11), "the main stream only");
        assert_eq!(r.alloc, 0, "resident data takes no clusters");
        assert_eq!(r.names.len(), 1, "the 8.3 name is left out");
        let (parent, start, end) = r.names[0];
        assert_eq!(parent, refer(16, 1));
        assert_eq!(String::from_utf16(&r.units[start..end]).unwrap(), "Long File Name.txt");
        assert_eq!(unix_seconds(r.modified.unwrap()), 1_700_000_000);
        let mut packed = record(FILE, 1, 0, &[standard(0), non_resident(DATA, "", 1_000_000, 1_048_576, Some(0), &[0x01, 0x80])]);
        assert!(fixup(&mut packed) && parse(&packed, &mut r, false, CLUSTER));
        assert_eq!((r.size, r.alloc), (Some(1_000_000), 0), "a cloud placeholder: sparse, nothing on the disk");
    }
    /// A small volume image: the table in two pieces (clusters 4–7 and
    /// 16–17, 24 records), so reading it has to follow its runs.
    fn image() -> Vec<u8> {
        let mut disk = vec![0u8; 18 * CLUSTER as usize];
        disk[..4096].copy_from_slice(&boot(4));
        let mut records: Vec<(u64, Vec<u8>)> = Vec::new();
        let table = 24 * RECORD as u64;
        records.push((0, record(FILE, 1, 0, &[standard(0x6), name(refer(5, 5), "$MFT", 3), non_resident(DATA, "", table, 6 * CLUSTER, None, &[0x11, 0x04, 0x04, 0x11, 0x02, 0x0C])])));
        records.push((5, record(FOLDER, 5, 0, &[standard(0x6), name(refer(5, 5), ".", 3)])));
        // A file whose data is described in an extension record (23).
        records.push((12, record(FILE, 1, 0, &[standard(0x20), name(refer(16, 1), "movie.mkv", 1)])));
        // 1222 clusters (0x04C6) from cluster 1: 5,005,312 bytes.
        records.push((23, record(FILE, 1, 12, &[non_resident(DATA, "", 5_000_000, 5_005_312, None, &[0x12, 0xC6, 0x04, 0x01])])));
        // A damaged name must not name another path.
        records.push((11, record(FILE, 1, 0, &[standard(0x20), name(refer(16, 1), r"..\..\Windows\evil.dll", 1), resident(DATA, "", b"x")])));
        // The bad cluster file: its stream "allocates" the whole volume as a hole.
        records.push((8, record(FILE, 1, 0, &[standard(0x6), name(refer(5, 5), "$BadClus", 3), non_resident(DATA, "$Bad", 931 << 30, 931 << 30, None, &[0x04, 0x00, 0x00, 0x00, 0x0F])])));
        // A name left from a folder 16 that was deleted and reused since.
        records.push((13, record(FILE, 1, 0, &[standard(0x20), name(refer(16, 9), "stale.txt", 1), resident(DATA, "", b"x")])));
        // A junction.
        let mut junction = vec![0u8; 16];
        junction[..4].copy_from_slice(&0xA000_0003u32.to_le_bytes());
        records.push((14, record(FOLDER, 1, 0, &[standard(0x410), name(refer(5, 5), "Link", 1), resident(REPARSE_POINT, "", &junction)])));
        // A file compressed by Windows (WOF): its data is in a named stream.
        let mut wof = vec![0u8; 16];
        wof[..4].copy_from_slice(&0x8000_0017u32.to_le_bytes());
        records.push((15, record(FILE, 1, 0, &[
            standard(0x420),
            name(refer(20, 1), "wof.exe", 1),
            non_resident(DATA, "", 1_000_000, 1_003_520, Some(0), &[0x01, 0xF5]),
            non_resident(DATA, "WofCompressedData", 400_000, 401_408, None, &[0x11, 0x62, 0x02]),
            resident(REPARSE_POINT, "", &wof),
        ])));
        records.push((16, record(FOLDER, 1, 0, &[standard(0), name(refer(5, 5), "Users", 1), non_resident(INDEX_ALLOCATION, "$I30", 4096, 4096, None, &[0x11, 0x01, 0x09])])));
        records.push((17, record(FILE, 1, 0, &[standard(0x20), name(refer(16, 1), "notes.txt", 1), name(refer(16, 1), "NOTES~1.TXT", 2), resident(DATA, "", b"hello world")])));
        records.push((18, record(FILE, 1, 0, &[standard(0x20), name(refer(16, 1), "big.bin", 1), non_resident(DATA, "", 10_000, 12_288, None, &[0x11, 0x03, 0x0A])])));
        // One file, two names: WinSxS's comes first in the record.
        records.push((19, record(FILE, 1, 0, &[standard(0x20), name(refer(21, 1), "shared.dll", 1), name(refer(20, 1), "shared.dll", 1), non_resident(DATA, "", 50_000, 53_248, None, &[0x11, 0x0D, 0x0E])])));
        records.push((20, record(FOLDER, 1, 0, &[standard(0), name(refer(5, 5), "Windows", 1)])));
        records.push((21, record(FOLDER, 1, 0, &[standard(0), name(refer(20, 1), "WinSxS", 1)])));
        // Deleted.
        records.push((22, record(0, 1, 0, &[standard(0x20), name(refer(16, 1), "gone.txt", 1)])));
        for (number, bytes) in records {
            let byte = number * RECORD as u64;
            let (cluster, within) = (byte / CLUSTER, byte % CLUSTER);
            let lcn = if cluster < 4 { 4 + cluster } else { 16 + cluster - 4 };
            let at = (lcn * CLUSTER + within) as usize;
            disk[at..at + RECORD].copy_from_slice(&bytes);
        }
        disk
    }
    fn snapshot() -> Snapshot {
        read_table(image().as_slice()).unwrap()
    }
    fn name_of(s: &Snapshot, link: usize) -> String {
        let at: usize = s.links[..link].iter().map(|l| l.name_len as usize).sum();
        String::from_utf8(s.names[at..at + s.links[link].name_len as usize].to_vec()).unwrap()
    }
    #[test]
    fn a_fragmented_table_is_read_record_by_record() {
        let s = snapshot();
        let indexes: Vec<u32> = s.files.iter().map(|f| f.index).collect();
        assert_eq!(indexes, [0, 5, 8, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21], "the deleted record and the extension are not files");
        let names: Vec<String> = (0..s.links.len()).map(|i| name_of(&s, i)).collect();
        assert!(names.contains(&"movie.mkv".to_string()));
        assert!(!names.contains(&"stale.txt".to_string()), "its folder was reused");
        assert!(!names.contains(&".".to_string()), "the root is not in itself");
        assert!(!names.iter().any(|n| n.contains('~')), "no 8.3 names");
        assert!(!names.iter().any(|n| n.contains("evil")), "a name with a path in it is dropped");
        let bad = s.files.iter().find(|f| f.index == 8).unwrap();
        assert_eq!(bad.alloc, 0, "a hole takes nothing, whatever the header says");
        assert_eq!(names.iter().filter(|n| *n == "shared.dll").count(), 2);
        let movie = s.files.iter().find(|f| f.index == 12).unwrap();
        assert_eq!((movie.size, movie.alloc), (5_000_000, 5_005_312), "from its extension record");
        let wof = s.files.iter().find(|f| f.index == 15).unwrap();
        assert_eq!((wof.size, wof.alloc), (1_000_000, 401_408), "on the disk: the compressed stream");
        assert_eq!(Snapshot::decode(&s.encode()).unwrap(), s);
    }
    #[test]
    fn only_names_windows_could_give_are_kept() {
        for good in ["a.txt", "$MFT", "Ελληνικά 📦", ".gitignore", "a..b", "x y"] {
            assert!(plain(good.as_bytes()), "{good}");
        }
        for bad in ["", ".", "..", r"..\x", "a/b", "c:d", "a\0b", "tab\there", "what?", "a|b", "\"q\""] {
            assert!(!plain(bad.as_bytes()), "{bad:?}");
        }
    }
    #[test]
    fn a_damaged_snapshot_is_refused() {
        let bytes = snapshot().encode();
        assert!(Snapshot::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut wrong = bytes.clone();
        wrong[0] = b'X';
        assert!(Snapshot::decode(&wrong).is_err());
        assert!(Snapshot::decode(&[]).is_err());
    }
    fn find(tree: &Tree, path: &str) -> u32 {
        let mut at = 0;
        for part in path.split('\\') {
            at = (0..tree.len() as u32).find(|&i| tree.node(i).parent == at && tree.name(i) == part).unwrap_or_else(|| panic!("{path}"));
        }
        at
    }
    #[test]
    fn the_tree_counts_a_hard_link_once_where_it_is_met_first() {
        let cancel = AtomicBool::new(false);
        let tree = into_tree(&snapshot(), 7, PathBuf::from(r"C:\"), &cancel, |_| {}).unwrap().unwrap();
        assert_eq!(tree.scan_id, 7);
        let shown = find(&tree, r"Windows\shared.dll");
        let other = find(&tree, r"Windows\WinSxS\shared.dll");
        assert_eq!(tree.size(shown, SizeMode::Size), 50_000);
        assert_eq!(tree.node(shown).flags & flag::HARD_LINK, 0);
        assert_eq!(tree.size(other, SizeMode::Size), 0);
        assert_ne!(tree.node(other).flags & flag::HARD_LINK, 0);
        assert_eq!(tree.node(find(&tree, "Link")).kind, Kind::Link);
        assert_eq!(tree.node(find(&tree, r"Windows\wof.exe")).kind, Kind::File);
        let mft = find(&tree, "$MFT");
        assert_ne!(tree.node(mft).flags & flag::SYSTEM, 0);
        assert_eq!(tree.size(mft, SizeMode::Alloc), 6 * CLUSTER);
        let users = find(&tree, "Users");
        assert_eq!(tree.size(users, SizeMode::Size), 11 + 10_000 + 5_000_000);
        assert_eq!(tree.size(users, SizeMode::Alloc), 4096 + 12_288 + 5_005_312, "the folder's index counts on the disk");
        assert_eq!(tree.files(0), 8, "$MFT, $BadClus, three in Users, wof.exe and both names of shared.dll");
        assert_eq!(tree.size(0, SizeMode::Size), 24 * RECORD as u64 + 11 + 10_000 + 5_000_000 + 1_000_000 + 50_000);
    }
    #[test]
    fn building_the_tree_stops_when_cancelled() {
        let cancel = AtomicBool::new(true);
        assert!(into_tree(&snapshot(), 1, PathBuf::from(r"C:\"), &cancel, |_| {}).unwrap().is_none());
    }
    /// Reads C: as the fast scan does, and compares with the walk. Needs an
    /// administrator: `cargo test mft_matches_the_walk -- --ignored --nocapture`
    /// from an elevated terminal (`MYLE_MFT_DRIVE` picks another drive).
    #[test]
    #[ignore]
    fn mft_matches_the_walk() {
        let letter = std::env::var("MYLE_MFT_DRIVE").ok().and_then(|d| d.chars().next()).unwrap_or('C');
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let snapshot = read_volume(letter).unwrap();
        let read = started.elapsed();
        let root = PathBuf::from(format!(r"{letter}:\"));
        let fast = into_tree(&snapshot, 1, root.clone(), &cancel, |_| {}).unwrap().unwrap();
        let built = started.elapsed();
        let started = Instant::now();
        let walk = super::super::scan::scan(&super::super::tree::long_path(&root), 2, &cancel, |_| {}).unwrap().unwrap();
        let walked = started.elapsed();
        let gb = |bytes: u64| bytes as f64 / 1e9;
        let mut names = vec![0u64; snapshot.files.len()];
        for link in &snapshot.links {
            names[link.file as usize] += 1;
        }
        let once: u64 = snapshot.files.iter().zip(&names).filter(|(f, n)| !f.dir && **n > 1).map(|(f, n)| f.size * (n - 1)).sum();
        println!("{letter}: hard links counted once: {:.2} GB the walk counts again", gb(once));
        println!(
            "{letter}: file table {:.1?} (+ tree {:.1?}): {} files, {:.2} GB, {:.2} GB on disk; walk {:.1?}: {} files, {:.2} GB, {:.2} GB on disk, {} folders unread",
            read,
            built - read,
            fast.files(0),
            gb(fast.size(0, SizeMode::Size)),
            gb(fast.size(0, SizeMode::Alloc)),
            walked,
            walk.files(0),
            gb(walk.size(0, SizeMode::Size)),
            gb(walk.size(0, SizeMode::Alloc)),
            walk.inaccessible.len(),
        );
        assert!(fast.files(0) >= walk.files(0) * 9 / 10, "the table sees at least what the walk sees");
    }
}
