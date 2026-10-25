//! The finished scan, kept in memory as one flat list of nodes.
//!
//! A node is 40 bytes plus its name, which lives in one shared buffer. A
//! folder's children are added together, so they sit next to each other and
//! the folder only keeps where they start and how many there are. Folder
//! totals are summed once the walk is over: children always come after their
//! folder in the list, so one pass from the end adds every node to its
//! parent after its own children were added to it.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// No node: the root's parent, a file's folder index.
pub const NONE: u32 = u32::MAX;
/// How many extensions get their own colour; the rest share one.
pub const RANKED: usize = 20;
/// The colour rank of everything outside the first `RANKED`.
pub const UNRANKED: u8 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Dir = 0,
    File = 1,
    /// A junction, symbolic link or mount point: listed, never followed.
    Link = 2,
    /// A cloud placeholder (OneDrive and others): counted, never opened.
    Cloud = 3,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Dir => "dir",
            Kind::File => "file",
            Kind::Link => "link",
            Kind::Cloud => "cloud",
        }
    }

    fn is_file(self) -> bool {
        matches!(self, Kind::File | Kind::Cloud)
    }
}

pub mod flag {
    /// A folder that could not be listed.
    pub const INACCESSIBLE: u8 = 1;
    /// FILE_ATTRIBUTE_SYSTEM.
    pub const SYSTEM: u8 = 2;
    /// Moved to the Recycle Bin (with everything inside it).
    pub const REMOVED: u8 = 4;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SizeMode {
    /// The logical size (EndOfFile).
    Size,
    /// Size on disk (AllocationSize).
    Alloc,
}

impl SizeMode {
    fn slot(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    Size,
    Name,
    Files,
    Modified,
}

#[derive(Clone, Copy)]
pub struct Node {
    pub size: u64,
    pub alloc: u64,
    pub parent: u32,
    name_at: u32,
    /// A folder's index in `folders`, or `NONE`.
    folder: u32,
    /// Date modified, seconds since 1970.
    pub modified: u32,
    name_len: u16,
    pub ext: u16,
    pub kind: Kind,
    pub flags: u8,
}

#[derive(Clone, Copy, Default)]
struct Folder {
    first: u32,
    count: u32,
    /// Files and folders anywhere below, once the walk is over.
    files: u32,
    dirs: u32,
}

/// One entry of a folder listing, as the scanner read it.
pub struct Entry {
    /// Its name's length in `Listing::names`, where the names follow each other.
    pub name_len: u32,
    pub kind: Kind,
    pub size: u64,
    pub alloc: u64,
    pub modified: u32,
    pub system: bool,
}

/// A whole folder's entries; names are WTF-8 (`OsStr::as_encoded_bytes`).
#[derive(Default)]
pub struct Listing {
    pub names: Vec<u8>,
    pub entries: Vec<Entry>,
}

impl Listing {
    #[cfg(test)]
    pub fn push(&mut self, name: &[u8], entry: Entry) {
        self.names.extend_from_slice(name);
        self.entries.push(entry);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtStat {
    /// Without the dot; "" for files without one, and for "Other".
    pub ext: String,
    pub other: bool,
    pub size: u64,
    pub alloc: u64,
    pub count: u64,
    /// The colour, the same as in the treemap.
    pub rank: u8,
}

/// A pruned subtree for drawing, as flat arrays: index 0 is the node asked
/// for, `parent` points into the same arrays.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Treemap {
    pub scan_id: u64,
    /// `NONE` for a "smaller items" block.
    pub ids: Vec<u32>,
    pub parent: Vec<i32>,
    pub size: Vec<u64>,
    /// 0 folder, 1 file, 2 link, 3 cloud, 4 smaller items.
    pub kind: Vec<u8>,
    /// Index into `exts` (0: none).
    pub ext: Vec<u16>,
    pub names: Vec<String>,
    pub exts: Vec<String>,
    /// The colour rank of each of `exts`.
    pub ranks: Vec<u8>,
    /// The node cap was reached before every folder was opened.
    pub truncated: bool,
}

pub const SMALLER: u8 = 4;

/// One page of a folder's children.
pub struct Page {
    pub ids: Vec<u32>,
    pub total: u32,
    pub rest_count: u32,
    pub rest_size: u64,
    pub rest_alloc: u64,
}

pub struct Tree {
    pub scan_id: u64,
    /// The scanned folder, as Windows shows it (no `\\?\`).
    pub root: PathBuf,
    nodes: Vec<Node>,
    folders: Vec<Folder>,
    names: Vec<u8>,
    exts: Vec<String>,
    ext_ids: HashMap<String, u16>,
    pub inaccessible: Vec<u32>,
    /// Children sorted by size, largest first, per size mode: a folder's
    /// sorted children are at the same positions as its id range.
    order: [Option<Vec<u32>>; 2],
    top: [Option<Vec<u32>>; 2],
    ranks: [Option<Vec<u8>>; 2],
}

impl Tree {
    pub fn new(scan_id: u64, root: PathBuf, modified: u32) -> Self {
        let name = root.as_os_str().as_encoded_bytes().to_vec();
        let mut tree = Self {
            scan_id,
            root,
            nodes: Vec::new(),
            folders: vec![Folder::default()],
            names: Vec::new(),
            exts: vec![String::new()],
            ext_ids: HashMap::new(),
            inaccessible: Vec::new(),
            order: [None, None],
            top: [None, None],
            ranks: [None, None],
        };
        let name_at = tree.store_name(&name);
        tree.nodes.push(Node {
            size: 0,
            alloc: 0,
            parent: NONE,
            name_at,
            folder: 0,
            modified,
            name_len: name.len().min(u16::MAX as usize) as u16,
            ext: 0,
            kind: Kind::Dir,
            flags: 0,
        });
        tree
    }

    fn store_name(&mut self, name: &[u8]) -> u32 {
        let at = self.names.len() as u32;
        self.names.extend_from_slice(&name[..name.len().min(u16::MAX as usize)]);
        at
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Adds a folder's listing as its children and returns their ids.
    pub fn add_children(&mut self, parent: u32, listing: &Listing) -> std::ops::Range<u32> {
        let first = self.nodes.len() as u32;
        let mut at = 0usize;
        for entry in &listing.entries {
            let name = &listing.names[at..at + entry.name_len as usize];
            at += entry.name_len as usize;
            let name_at = self.store_name(name);
            let ext = if entry.kind.is_file() { self.ext_id(name) } else { 0 };
            let folder = if entry.kind == Kind::Dir {
                self.folders.push(Folder::default());
                (self.folders.len() - 1) as u32
            } else {
                NONE
            };
            let (size, alloc) = if entry.kind == Kind::Link { (0, 0) } else { (entry.size, entry.alloc) };
            self.nodes.push(Node {
                size,
                alloc,
                parent,
                name_at,
                folder,
                modified: entry.modified,
                name_len: name.len().min(u16::MAX as usize) as u16,
                ext,
                kind: entry.kind,
                flags: if entry.system { flag::SYSTEM } else { 0 },
            });
        }
        let end = self.nodes.len() as u32;
        if let Some(folder) = self.folder_mut(parent) {
            folder.first = first;
            folder.count = end - first;
        }
        first..end
    }

    fn ext_id(&mut self, name: &[u8]) -> u16 {
        let Some(ext) = extension(name) else { return 0 };
        if let Some(&id) = self.ext_ids.get(&ext) {
            return id;
        }
        if self.exts.len() > u16::MAX as usize {
            return 0;
        }
        let id = self.exts.len() as u16;
        self.exts.push(ext.clone());
        self.ext_ids.insert(ext, id);
        id
    }

    pub fn mark_inaccessible(&mut self, id: u32) {
        if let Some(node) = self.nodes.get_mut(id as usize) {
            node.flags |= flag::INACCESSIBLE;
            self.inaccessible.push(id);
        }
    }

    /// Sums every folder bottom-up, once the walk is over.
    pub fn finish(&mut self) {
        self.ext_ids = HashMap::new();
        // Nothing is added any more: give back what the lists grew ahead.
        self.nodes.shrink_to_fit();
        self.folders.shrink_to_fit();
        self.names.shrink_to_fit();
        for i in (1..self.nodes.len()).rev() {
            let node = self.nodes[i];
            let (files, dirs) = match node.kind {
                Kind::Dir => {
                    let below = self.folders[node.folder as usize];
                    (below.files, below.dirs + 1)
                }
                Kind::File | Kind::Cloud => (1, 0),
                Kind::Link => (0, 0),
            };
            let parent = &mut self.nodes[node.parent as usize];
            parent.size += node.size;
            parent.alloc += node.alloc;
            let folder = &mut self.folders[parent.folder as usize];
            folder.files += files;
            folder.dirs += dirs;
        }
    }

    fn folder(&self, id: u32) -> Option<Folder> {
        let node = self.nodes.get(id as usize)?;
        (node.folder != NONE).then(|| self.folders[node.folder as usize])
    }

    fn folder_mut(&mut self, id: u32) -> Option<&mut Folder> {
        let folder = self.nodes.get(id as usize)?.folder;
        (folder != NONE).then(|| &mut self.folders[folder as usize])
    }

    /// Checks that `id` belongs to this scan and is still there.
    pub fn check(&self, scan_id: u64, id: u32) -> Result<(), String> {
        if scan_id != self.scan_id {
            return Err(STALE.into());
        }
        match self.nodes.get(id as usize) {
            Some(node) if node.flags & flag::REMOVED == 0 => Ok(()),
            Some(_) => Err("That item was moved to the Recycle Bin.".into()),
            None => Err(STALE.into()),
        }
    }

    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn name(&self, id: u32) -> &OsStr {
        let node = &self.nodes[id as usize];
        let bytes = &self.names[node.name_at as usize..node.name_at as usize + node.name_len as usize];
        // SAFETY: every name was stored from `OsStr::as_encoded_bytes` (or
        // from valid UTF-8, which is the same) by this same program.
        unsafe { OsStr::from_encoded_bytes_unchecked(bytes) }
    }

    pub fn size(&self, id: u32, mode: SizeMode) -> u64 {
        let node = &self.nodes[id as usize];
        match mode {
            SizeMode::Size => node.size,
            SizeMode::Alloc => node.alloc,
        }
    }

    pub fn files(&self, id: u32) -> u32 {
        match self.nodes[id as usize].kind {
            Kind::Dir => self.folder(id).map_or(0, |f| f.files),
            Kind::File | Kind::Cloud => 1,
            Kind::Link => 0,
        }
    }

    pub fn dirs(&self, id: u32) -> u32 {
        self.folder(id).map_or(0, |f| f.dirs)
    }

    pub fn is_removed(&self, id: u32) -> bool {
        self.nodes[id as usize].flags & flag::REMOVED != 0
    }

    pub fn has_children(&self, id: u32) -> bool {
        self.live_children(id).next().is_some()
    }

    fn child_range(&self, id: u32) -> std::ops::Range<u32> {
        self.folder(id).map_or(0..0, |f| f.first..f.first + f.count)
    }

    fn live_children(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        self.child_range(id).filter(|&c| !self.is_removed(c))
    }

    /// The scanned folder down to `id`.
    pub fn trail(&self, id: u32) -> Vec<u32> {
        let mut trail = vec![id];
        let mut at = id;
        while self.nodes[at as usize].parent != NONE {
            at = self.nodes[at as usize].parent;
            trail.push(at);
        }
        trail.reverse();
        trail
    }

    /// The item's path as Windows shows it.
    pub fn path(&self, id: u32) -> PathBuf {
        let trail = self.trail(id);
        let mut path = self.root.clone();
        for &step in &trail[1..] {
            path.push(self.name(step));
        }
        path
    }

    // ------------------------------------------------------------------
    // Children

    fn sorted(&mut self, mode: SizeMode) -> &[u32] {
        if self.order[mode.slot()].is_none() {
            let mut order: Vec<u32> = (0..self.nodes.len() as u32).collect();
            for folder in &self.folders {
                let range = folder.first as usize..(folder.first + folder.count) as usize;
                sort_by_size(&mut order[range], &self.nodes, mode);
            }
            self.order[mode.slot()] = Some(order);
        }
        self.order[mode.slot()].as_deref().unwrap_or_default()
    }

    /// A page of `id`'s children, plus what is left after it.
    pub fn children(&mut self, id: u32, offset: usize, limit: usize, sort: SortKey, mode: SizeMode) -> Page {
        let range = self.child_range(id);
        let mut ids: Vec<u32> = if sort == SortKey::Size {
            self.sorted(mode)[range.start as usize..range.end as usize].to_vec()
        } else {
            range.collect()
        };
        ids.retain(|&c| !self.is_removed(c));
        match sort {
            SortKey::Size => {}
            SortKey::Name => ids.sort_by_cached_key(|&c| self.name(c).to_string_lossy().to_lowercase()),
            SortKey::Files => ids.sort_by_key(|&c| (Reverse(self.files(c)), Reverse(self.size(c, mode)))),
            SortKey::Modified => ids.sort_by_key(|&c| (Reverse(self.nodes[c as usize].modified), c)),
        }
        let total = ids.len() as u32;
        let start = offset.min(ids.len());
        let end = (start + limit).min(ids.len());
        let rest = &ids[end..];
        Page {
            rest_count: rest.len() as u32,
            rest_size: rest.iter().map(|&c| self.nodes[c as usize].size).sum(),
            rest_alloc: rest.iter().map(|&c| self.nodes[c as usize].alloc).sum(),
            ids: ids[start..end].to_vec(),
            total,
        }
    }

    // ------------------------------------------------------------------
    // Largest files and file types

    /// The largest files of the whole scan, largest first. Without a filter
    /// the answer is worked out once and kept.
    pub fn top_files(&mut self, mode: SizeMode, limit: usize, filter: Option<&str>) -> Vec<u32> {
        let filter = filter.map(str::trim).filter(|f| !f.is_empty()).map(str::to_lowercase);
        if filter.is_none()
            && let Some(top) = &self.top[mode.slot()]
        {
            return top[..limit.min(top.len())].to_vec();
        }
        let keep = if filter.is_none() { TOP_CACHED.max(limit) } else { limit };
        let mut heap: BinaryHeap<Reverse<(u64, Reverse<u32>)>> = BinaryHeap::with_capacity(keep + 1);
        for (i, node) in self.nodes.iter().enumerate() {
            if !node.kind.is_file() || node.flags & flag::REMOVED != 0 {
                continue;
            }
            let size = match mode {
                SizeMode::Size => node.size,
                SizeMode::Alloc => node.alloc,
            };
            if heap.len() == keep && heap.peek().is_some_and(|Reverse((smallest, _))| size <= *smallest) {
                continue;
            }
            if let Some(needle) = &filter
                && !contains_ignore_case(self.name(i as u32), needle)
            {
                continue;
            }
            heap.push(Reverse((size, Reverse(i as u32))));
            if heap.len() > keep {
                heap.pop();
            }
        }
        let mut top: Vec<(u64, Reverse<u32>)> = heap.into_iter().map(|Reverse(item)| item).collect();
        top.sort_by(|a, b| b.cmp(a));
        let ids: Vec<u32> = top.into_iter().map(|(_, Reverse(id))| id).collect();
        if filter.is_none() {
            self.top[mode.slot()] = Some(ids.clone());
        }
        ids.into_iter().take(limit).collect()
    }

    fn ext_totals(&self, under: u32) -> Vec<(u64, u64, u64)> {
        let mut totals = vec![(0u64, 0u64, 0u64); self.exts.len()];
        let mut add = |node: &Node| {
            if node.kind.is_file() && node.flags & flag::REMOVED == 0 {
                let t = &mut totals[node.ext as usize];
                t.0 += node.size;
                t.1 += node.alloc;
                t.2 += 1;
            }
        };
        if under == 0 {
            self.nodes.iter().for_each(&mut add);
        } else {
            let mut stack = vec![under];
            while let Some(at) = stack.pop() {
                for child in self.live_children(at) {
                    let node = &self.nodes[child as usize];
                    add(node);
                    if node.kind == Kind::Dir {
                        stack.push(child);
                    }
                }
            }
        }
        totals
    }

    /// The colour rank of every extension: the `RANKED` largest of the whole
    /// scan get one each, in order. Kept for the scan, so colours stay put
    /// after a delete.
    pub fn ranks(&mut self, mode: SizeMode) -> &[u8] {
        if self.ranks[mode.slot()].is_none() {
            let totals = self.ext_totals(0);
            let mut by_size: Vec<usize> = (0..totals.len()).filter(|&e| totals[e].2 > 0).collect();
            by_size.sort_by_key(|&e| (Reverse(pick(totals[e], mode)), e));
            let mut ranks = vec![UNRANKED; totals.len()];
            for (rank, &e) in by_size.iter().take(RANKED).enumerate() {
                ranks[e] = rank as u8;
            }
            self.ranks[mode.slot()] = Some(ranks);
        }
        self.ranks[mode.slot()].as_deref().unwrap_or_default()
    }

    /// Files by extension below `under`: the largest `RANKED`, then "Other".
    pub fn extensions(&mut self, under: u32, mode: SizeMode) -> Vec<ExtStat> {
        let totals = self.ext_totals(under);
        let ranks = self.ranks(mode).to_vec();
        let mut used: Vec<usize> = (0..totals.len()).filter(|&e| totals[e].2 > 0).collect();
        used.sort_by_key(|&e| (Reverse(pick(totals[e], mode)), e));
        let mut stats: Vec<ExtStat> = used
            .iter()
            .take(RANKED)
            .map(|&e| ExtStat {
                ext: self.exts[e].clone(),
                other: false,
                size: totals[e].0,
                alloc: totals[e].1,
                count: totals[e].2,
                rank: ranks[e],
            })
            .collect();
        let rest = &used[used.len().min(RANKED)..];
        if !rest.is_empty() {
            stats.push(ExtStat {
                ext: String::new(),
                other: true,
                size: rest.iter().map(|&e| totals[e].0).sum(),
                alloc: rest.iter().map(|&e| totals[e].1).sum(),
                count: rest.iter().map(|&e| totals[e].2).sum(),
                rank: UNRANKED,
            });
        }
        stats
    }

    pub fn ext_of(&self, id: u32) -> &str {
        &self.exts[self.nodes[id as usize].ext as usize]
    }

    // ------------------------------------------------------------------
    // Treemap

    /// The subtree of `id` for the treemap. Big folders are opened first, so
    /// when `cap` is reached the detail went where the space is. Entries
    /// under `min_bytes` are merged, per folder, into one block.
    pub fn treemap(&mut self, id: u32, depth: u8, min_bytes: u64, mode: SizeMode, cap: usize) -> Treemap {
        let ranks = self.ranks(mode).to_vec();
        self.sorted(mode);
        let order = self.order[mode.slot()].take().unwrap_or_default();
        let mut out = Treemap {
            scan_id: self.scan_id,
            ids: Vec::new(),
            parent: Vec::new(),
            size: Vec::new(),
            kind: Vec::new(),
            ext: Vec::new(),
            names: Vec::new(),
            exts: vec![String::new()],
            ranks: vec![UNRANKED],
            truncated: false,
        };
        let mut local_ext: HashMap<u16, u16> = HashMap::new();
        let min_bytes = min_bytes.max(1);
        self.push_treemap(&mut out, &mut local_ext, &ranks, id, -1, mode);
        let mut heap = BinaryHeap::new();
        if self.nodes[id as usize].kind == Kind::Dir && depth > 0 {
            heap.push((self.size(id, mode), Reverse(0usize), id, 1u8));
        }
        while let Some((_, Reverse(at), node, level)) = heap.pop() {
            if out.ids.len() >= cap {
                out.truncated = true;
                break;
            }
            let range = self.child_range(node);
            let mut rest = (0u32, 0u64);
            for &child in &order[range.start as usize..range.end as usize] {
                if self.is_removed(child) {
                    continue;
                }
                let size = self.size(child, mode);
                if size < min_bytes || out.ids.len() >= cap {
                    rest.0 += 1;
                    rest.1 += size;
                    continue;
                }
                let index = out.ids.len();
                self.push_treemap(&mut out, &mut local_ext, &ranks, child, at as i32, mode);
                if self.nodes[child as usize].kind == Kind::Dir && level < depth && self.has_children(child) {
                    heap.push((size, Reverse(index), child, level + 1));
                }
            }
            if rest.0 > 0 && rest.1 > 0 {
                out.ids.push(NONE);
                out.parent.push(at as i32);
                out.size.push(rest.1);
                out.kind.push(SMALLER);
                out.ext.push(0);
                out.names.push(format!("{} smaller item{}", rest.0, if rest.0 == 1 { "" } else { "s" }));
            }
        }
        self.order[mode.slot()] = Some(order);
        out
    }

    fn push_treemap(
        &self,
        out: &mut Treemap,
        local_ext: &mut HashMap<u16, u16>,
        ranks: &[u8],
        id: u32,
        parent: i32,
        mode: SizeMode,
    ) {
        let node = &self.nodes[id as usize];
        let ext = if node.ext == 0 {
            0
        } else {
            *local_ext.entry(node.ext).or_insert_with(|| {
                out.exts.push(self.exts[node.ext as usize].clone());
                out.ranks.push(ranks[node.ext as usize]);
                (out.exts.len() - 1) as u16
            })
        };
        out.ids.push(id);
        out.parent.push(parent);
        out.size.push(self.size(id, mode));
        out.kind.push(node.kind as u8);
        out.ext.push(ext);
        out.names.push(self.name(id).to_string_lossy().into_owned());
    }

    // ------------------------------------------------------------------
    // Removing

    /// Takes an item out after it went to the Recycle Bin: its sizes and
    /// counts come off every folder above it, and it and everything inside
    /// it are marked removed. Returns its (size, size on disk).
    pub fn remove(&mut self, id: u32) -> (u64, u64) {
        if id == 0 || self.is_removed(id) {
            return (0, 0);
        }
        let node = self.nodes[id as usize];
        let (files, dirs) = match node.kind {
            Kind::Dir => (self.files(id), self.dirs(id) + 1),
            Kind::File | Kind::Cloud => (1, 0),
            Kind::Link => (0, 0),
        };
        let mut at = node.parent;
        while at != NONE {
            let parent = &mut self.nodes[at as usize];
            parent.size -= node.size.min(parent.size);
            parent.alloc -= node.alloc.min(parent.alloc);
            let up = parent.parent;
            if let Some(folder) = self.folder_mut(at) {
                folder.files -= files.min(folder.files);
                folder.dirs -= dirs.min(folder.dirs);
            }
            at = up;
        }
        let mut stack = vec![id];
        while let Some(at) = stack.pop() {
            self.nodes[at as usize].flags |= flag::REMOVED;
            stack.extend(self.child_range(at));
        }
        // The folders above shrank: their place among their siblings may
        // have changed.
        let trail = self.trail(node.parent);
        for slot in 0..2 {
            let Some(mut order) = self.order[slot].take() else { continue };
            let mode = if slot == 0 { SizeMode::Size } else { SizeMode::Alloc };
            for &folder in &trail {
                let range = self.child_range(folder);
                sort_by_size(&mut order[range.start as usize..range.end as usize], &self.nodes, mode);
            }
            self.order[slot] = Some(order);
        }
        self.top = [None, None];
        (node.size, node.alloc)
    }

    /// Bytes held by the tree, for the measurements in the PR.
    pub fn memory(&self) -> usize {
        self.nodes.capacity() * size_of::<Node>()
            + self.folders.capacity() * size_of::<Folder>()
            + self.names.capacity()
            + self.exts.iter().map(|e| e.capacity() + size_of::<String>()).sum::<usize>()
            + self.order.iter().flatten().map(|o| o.capacity() * 4).sum::<usize>()
    }
}

pub const STALE: &str = "These results are from an earlier scan. Scan again.";
/// How many of the largest files are worked out and kept.
const TOP_CACHED: usize = 1000;

fn pick(total: (u64, u64, u64), mode: SizeMode) -> u64 {
    match mode {
        SizeMode::Size => total.0,
        SizeMode::Alloc => total.1,
    }
}

/// Largest first; equal sizes keep the listing's order.
fn sort_by_size(ids: &mut [u32], nodes: &[Node], mode: SizeMode) {
    ids.sort_unstable_by_key(|&c| {
        let node = &nodes[c as usize];
        let size = match mode {
            SizeMode::Size => node.size,
            SizeMode::Alloc => node.alloc,
        };
        (Reverse(size), c)
    });
}

/// "mp4" for "Clip.MP4". None for names without one, ".gitignore", and
/// anything after the dot that does not look like an extension.
fn extension(name: &[u8]) -> Option<String> {
    let dot = name.iter().rposition(|&b| b == b'.')?;
    let ext = &name[dot + 1..];
    if dot == 0 || ext.is_empty() || ext.len() > 16 || !ext.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-') {
        return None;
    }
    Some(String::from_utf8_lossy(ext).to_ascii_lowercase())
}

fn contains_ignore_case(name: &OsStr, needle: &str) -> bool {
    let bytes = name.as_encoded_bytes();
    if needle.is_ascii() {
        let needle = needle.as_bytes();
        return needle.is_empty()
            || bytes.windows(needle.len()).any(|window| window.eq_ignore_ascii_case(needle));
    }
    name.to_string_lossy().to_lowercase().contains(needle)
}

/// Seconds since 1970 from a FILETIME (100 ns since 1601), kept in a u32.
pub fn unix_seconds(filetime: i64) -> u32 {
    const SINCE_1601: i64 = 11_644_473_600;
    (filetime / 10_000_000 - SINCE_1601).clamp(0, u32::MAX as i64) as u32
}

/// `C:\x` for `\\?\C:\x`, `\\server\share` for `\\?\UNC\server\share`.
pub fn display_path(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// The `\\?\` form, which has no length limit.
pub fn long_path(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if text.starts_with(r"\\?\") {
        path.to_path_buf()
    } else if let Some(rest) = text.strip_prefix(r"\\") {
        PathBuf::from(format!(r"\\?\UNC\{rest}"))
    } else {
        PathBuf::from(format!(r"\\?\{text}"))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn entry(kind: Kind, size: u64, alloc: u64) -> (Kind, u64, u64) {
        (kind, size, alloc)
    }

    /// A listing from (name, kind, size, size on disk).
    pub fn listing(items: &[(&str, (Kind, u64, u64))]) -> Listing {
        let mut listing = Listing::default();
        for (name, (kind, size, alloc)) in items {
            listing.push(
                name.as_bytes(),
                Entry { name_len: name.len() as u32, kind: *kind, size: *size, alloc: *alloc, modified: 1_000, system: false },
            );
        }
        listing
    }

    /// root
    ///   docs/ (a.pdf 100, b.txt 50, deep/ (c.pdf 400))
    ///   video.mp4 1000
    ///   empty/
    ///   link -> (not followed)
    ///   tiny.txt 1
    pub fn sample() -> Tree {
        let mut tree = Tree::new(7, PathBuf::from(r"C:\Data"), 0);
        let top = tree.add_children(
            0,
            &listing(&[
                ("docs", entry(Kind::Dir, 0, 0)),
                ("video.mp4", entry(Kind::File, 1000, 1024)),
                ("empty", entry(Kind::Dir, 0, 0)),
                ("link", entry(Kind::Link, 999, 999)),
                ("tiny.txt", entry(Kind::File, 1, 4096)),
            ]),
        );
        assert_eq!(top, 1..6);
        let docs = tree.add_children(
            1,
            &listing(&[("a.pdf", entry(Kind::File, 100, 4096)), ("b.txt", entry(Kind::File, 50, 4096)), ("deep", entry(Kind::Dir, 0, 0))]),
        );
        tree.add_children(3, &listing(&[]));
        tree.add_children(docs.end - 1, &listing(&[("c.pdf", entry(Kind::Cloud, 400, 0))]));
        tree.finish();
        tree
    }

    fn id_of(tree: &Tree, name: &str) -> u32 {
        (0..tree.len() as u32).find(|&i| tree.name(i) == name).unwrap()
    }

    #[test]
    fn a_node_stays_small() {
        assert_eq!(size_of::<Node>(), 40);
        assert_eq!(size_of::<Folder>(), 16);
    }

    #[test]
    fn folder_totals_are_summed_bottom_up() {
        let tree = sample();
        assert_eq!(tree.size(0, SizeMode::Size), 1000 + 1 + 100 + 50 + 400, "a link counts nothing");
        assert_eq!(tree.size(0, SizeMode::Alloc), 1024 + 4096 * 3);
        let docs = id_of(&tree, "docs");
        assert_eq!(tree.size(docs, SizeMode::Size), 550);
        assert_eq!(tree.files(0), 5);
        assert_eq!(tree.dirs(0), 3, "docs, deep and empty");
        assert_eq!(tree.files(docs), 3);
        assert_eq!(tree.dirs(docs), 1);
        assert_eq!(tree.path(id_of(&tree, "c.pdf")), PathBuf::from(r"C:\Data\docs\deep\c.pdf"));
        assert!(!tree.has_children(id_of(&tree, "empty")));
    }

    #[test]
    fn children_come_largest_first_with_the_rest_summed() {
        let mut tree = sample();
        let page = tree.children(0, 0, 2, SortKey::Size, SizeMode::Size);
        let names: Vec<_> = page.ids.iter().map(|&c| tree.name(c).to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["video.mp4", "docs"]);
        assert_eq!(page.total, 5);
        assert_eq!(page.rest_count, 3);
        assert_eq!(page.rest_size, 1, "tiny.txt; the link and the empty folder are 0");
        // Size on disk puts the 4 KB clusters first.
        let page = tree.children(0, 0, 1, SortKey::Size, SizeMode::Alloc);
        assert_eq!(tree.name(page.ids[0]), "docs");
        // The next page.
        let next = tree.children(0, 2, 2, SortKey::Size, SizeMode::Size);
        assert_eq!(next.ids.len(), 2);
        assert_eq!(next.rest_count, 1);
        let by_name = tree.children(0, 0, 10, SortKey::Name, SizeMode::Size);
        assert_eq!(tree.name(by_name.ids[0]), "docs");
        assert_eq!(tree.name(by_name.ids[4]), "video.mp4");
    }

    #[test]
    fn the_largest_files_and_a_filter() {
        let mut tree = sample();
        let top = tree.top_files(SizeMode::Size, 3, None);
        let names: Vec<_> = top.iter().map(|&c| tree.name(c).to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["video.mp4", "c.pdf", "a.pdf"]);
        let pdfs = tree.top_files(SizeMode::Size, 10, Some("PDF"));
        assert_eq!(pdfs.len(), 2);
        assert_eq!(tree.top_files(SizeMode::Size, 10, Some("nothing")).len(), 0);
    }

    #[test]
    fn extensions_are_counted_and_ranked_for_colours() {
        let mut tree = sample();
        let stats = tree.extensions(0, SizeMode::Size);
        let exts: Vec<_> = stats.iter().map(|s| (s.ext.as_str(), s.size, s.count, s.rank)).collect();
        assert_eq!(exts, [("mp4", 1000, 1, 0), ("pdf", 500, 2, 1), ("txt", 51, 2, 2)]);
        // Below docs only, with the scan's colours.
        let docs = id_of(&tree, "docs");
        let stats = tree.extensions(docs, SizeMode::Size);
        assert_eq!(stats[0].ext, "pdf");
        assert_eq!(stats[0].rank, 1);
    }

    #[test]
    fn only_the_first_ranked_extensions_get_a_colour() {
        let mut tree = Tree::new(1, PathBuf::from(r"D:\"), 0);
        let names: Vec<String> = (0..25).map(|i| format!("f{i}.e{i}")).collect();
        let items: Vec<(&str, (Kind, u64, u64))> =
            names.iter().enumerate().map(|(i, n)| (n.as_str(), entry(Kind::File, 100 - i as u64, 0))).collect();
        tree.add_children(0, &listing(&items));
        tree.finish();
        let stats = tree.extensions(0, SizeMode::Size);
        assert_eq!(stats.len(), RANKED + 1);
        assert!(stats[RANKED].other);
        assert_eq!(stats[RANKED].count, 5);
        assert_eq!(stats[RANKED].rank, UNRANKED);
    }

    #[test]
    fn removing_a_node_fixes_the_totals_up_the_tree() {
        let mut tree = sample();
        let deep = id_of(&tree, "deep");
        let docs = id_of(&tree, "docs");
        let _ = tree.children(0, 0, 10, SortKey::Size, SizeMode::Size);
        assert_eq!(tree.remove(deep), (400, 0));
        assert_eq!(tree.size(docs, SizeMode::Size), 150);
        assert_eq!(tree.size(0, SizeMode::Size), 1151);
        assert_eq!(tree.files(0), 4);
        assert_eq!(tree.dirs(0), 2);
        assert!(tree.is_removed(id_of(&tree, "c.pdf")), "and everything inside it");
        assert!(tree.check(7, deep).is_err());
        let page = tree.children(docs, 0, 10, SortKey::Size, SizeMode::Size);
        assert_eq!(page.ids.len(), 2);
        assert!(!tree.top_files(SizeMode::Size, 10, None).contains(&id_of(&tree, "c.pdf")));
        // Removing twice, or the root, takes nothing.
        assert_eq!(tree.remove(deep), (0, 0));
        assert_eq!(tree.remove(0), (0, 0));
    }

    #[test]
    fn removing_moves_a_folder_down_among_its_siblings() {
        let mut tree = sample();
        let docs = id_of(&tree, "docs");
        let a = id_of(&tree, "a.pdf");
        let page = tree.children(0, 0, 10, SortKey::Size, SizeMode::Alloc);
        assert_eq!(page.ids[0], docs);
        tree.remove(a);
        tree.remove(id_of(&tree, "b.txt"));
        let page = tree.children(0, 0, 10, SortKey::Size, SizeMode::Alloc);
        assert_eq!(tree.name(page.ids[0]), "tiny.txt", "4 KB on disk beats docs' 0 now");
    }

    #[test]
    fn a_stale_scan_id_is_refused() {
        let tree = sample();
        assert!(tree.check(7, 1).is_ok());
        assert_eq!(tree.check(8, 1).unwrap_err(), STALE);
        assert!(tree.check(7, 999).is_err());
    }

    #[test]
    fn the_treemap_merges_small_entries_and_respects_the_cap() {
        let mut tree = sample();
        let map = tree.treemap(0, 8, 60, SizeMode::Size, 20_000);
        assert_eq!(map.ids[0], 0);
        assert_eq!(map.parent[0], -1);
        // Root: video, docs, then "2 smaller items"? tiny.txt is 1 byte; the
        // link and the empty folder are 0, so the block holds 1 byte.
        let smaller: Vec<_> = map.names.iter().filter(|n| n.contains("smaller")).collect();
        assert!(smaller.iter().any(|n| n.starts_with("3 smaller items")), "{smaller:?}");
        let total_children: u64 = (0..map.ids.len()).filter(|&i| map.parent[i] == 0).map(|i| map.size[i]).sum();
        assert_eq!(total_children, map.size[0], "a folder's blocks add up to it");
        // b.txt (50) is below 60 inside docs.
        let docs_index = map.ids.iter().position(|&i| i == id_of(&tree, "docs")).unwrap() as i32;
        let in_docs: Vec<_> = (0..map.ids.len()).filter(|&i| map.parent[i] == docs_index).map(|i| map.names[i].clone()).collect();
        assert_eq!(in_docs, ["deep", "a.pdf", "1 smaller item"]);
        assert!(map.exts.contains(&"pdf".to_string()));
        assert!(!map.truncated);
        let capped = tree.treemap(0, 8, 1, SizeMode::Size, 3);
        assert!(capped.truncated);
        assert!(capped.ids.len() <= 4);
        let shallow = tree.treemap(0, 1, 1, SizeMode::Size, 20_000);
        assert!(!shallow.ids.contains(&id_of(&tree, "a.pdf")), "depth 1 stops at the root's children");
    }

    #[test]
    fn extensions_and_paths_are_read_from_names() {
        assert_eq!(extension(b"Clip.MP4").as_deref(), Some("mp4"));
        assert_eq!(extension(b"archive.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension(b".gitignore"), None);
        assert_eq!(extension(b"README"), None);
        assert_eq!(extension(b"name.with space"), None);
        assert_eq!(display_path(Path::new(r"\\?\C:\x")), PathBuf::from(r"C:\x"));
        assert_eq!(display_path(Path::new(r"\\?\UNC\srv\share\x")), PathBuf::from(r"\\srv\share\x"));
        assert_eq!(long_path(Path::new(r"C:\x")), PathBuf::from(r"\\?\C:\x"));
        assert_eq!(long_path(Path::new(r"\\srv\share")), PathBuf::from(r"\\?\UNC\srv\share"));
        assert_eq!(unix_seconds(116_444_736_000_000_000 + 10_000_000), 1);
        assert_eq!(unix_seconds(0), 0);
    }
}
