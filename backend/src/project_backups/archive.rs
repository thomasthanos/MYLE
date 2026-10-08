//! Writing a backup zip that is known to be whole before it gets its name.
//!
//! The zip is built in a local staging folder (cloud folders such as Google
//! Drive for desktop show a stale view of a file that is still uploading),
//! every entry is read back and checked against what was read from the
//! project (size and CRC-32), the project folder is walked once more to make
//! sure nothing is missing, and only then is the zip copied next to the other
//! backups under a `__partial__` name, re-read until its SHA-256 matches, and
//! renamed. The same steps and names as backup_projects 2.0.6.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::naming;
use super::rules::PARTIAL_PREFIX;
use super::walk::SourceFile;
use super::{Failure, Sink, Stage};
use crate::cloud::onedrive::PROVIDER_NOT_RUNNING;

/// Waits between re-reads of a copy at the destination (≈ 60 s in all).
pub(crate) const RETRY_DELAYS: &[u64] = &[300, 700, 1500, 3000, 5000, 8000, 12000, 15000, 15000];
/// An empty zip is only its 22-byte end record.
pub(crate) const MIN_ZIP_SIZE: u64 = 22;
const STAGING_MARGIN: u64 = 64 * 1024 * 1024;
const BUFFER: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EntryInfo {
    pub size: u64,
    pub crc: u32,
}

/// What went into a zip, as read from the source.
#[derive(Debug, Default)]
pub(crate) struct Written {
    pub manifest: BTreeMap<String, EntryInfo>,
    pub dirs: Vec<String>,
    pub total_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct Verified {
    pub zip_size: u64,
    /// The renamed zip showed its full size in time.
    pub final_check_ok: bool,
}

/// What a zip holds, read back from it.
#[derive(Debug, Default)]
pub(crate) struct ZipCheck {
    pub files: usize,
    pub dirs: usize,
    pub bytes: u64,
}

fn io_failure(context: String, error: io::Error, path: &Path) -> Failure {
    if error.raw_os_error() == Some(PROVIDER_NOT_RUNNING as i32) {
        return Failure::CloudOffline(path.to_path_buf());
    }
    Failure::Message(format!("{context}: {error}"))
}

fn dos_time(time: Option<SystemTime>) -> DateTime {
    let local = naming::local(time.unwrap_or_else(SystemTime::now));
    let year = local.year.clamp(1980, 2107) as u16;
    DateTime::from_date_and_time(
        year,
        local.month as u8,
        local.day as u8,
        local.hour as u8,
        local.minute as u8,
        local.second as u8,
    )
    .unwrap_or_default()
}

/// Streams `files` (and small in-memory `buffers`) into a new zip at `path`.
pub(crate) fn write_zip(
    path: &Path,
    files: &[SourceFile],
    empty_dirs: &[String],
    buffers: &[(String, Vec<u8>)],
    sink: &dyn Sink,
) -> Result<Written, Failure> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            Failure::Message(format!("Can't create \"{}\": {error}", path.display()))
        })?;
    let result = write_entries(file, files, empty_dirs, buffers, sink);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn write_entries(
    file: File,
    files: &[SourceFile],
    empty_dirs: &[String],
    buffers: &[(String, Vec<u8>)],
    sink: &dyn Sink,
) -> Result<Written, Failure> {
    let zip_error =
        |error: zip::result::ZipError| Failure::Message(format!("Can't write the zip: {error}"));
    let mut zip = ZipWriter::new(BufWriter::with_capacity(BUFFER, file));
    let mut written = Written::default();
    let total_bytes: u64 = files.iter().map(|file| file.size).sum::<u64>()
        + buffers
            .iter()
            .map(|(_, data)| data.len() as u64)
            .sum::<u64>();
    let total_files = (files.len() + buffers.len()) as u64;
    let mut done_bytes = 0u64;
    let mut buffer = vec![0u8; BUFFER];
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6))
        .unix_permissions(0o644);

    for (index, source) in files.iter().enumerate() {
        if sink.cancelled() {
            return Err(Failure::Cancelled);
        }
        let mut input = File::open(&source.abs).map_err(|error| {
            io_failure(format!("Can't read \"{}\"", source.rel), error, &source.abs)
        })?;
        let options = options
            .last_modified_time(dos_time(source.modified))
            .large_file(source.size >= 0xFFF0_0000);
        zip.start_file(source.rel.as_str(), options)
            .map_err(zip_error)?;
        let mut hasher = crc32fast::Hasher::new();
        let mut size = 0u64;
        loop {
            let read = input.read(&mut buffer).map_err(|error| {
                io_failure(format!("Can't read \"{}\"", source.rel), error, &source.abs)
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            zip.write_all(&buffer[..read])
                .map_err(|error| Failure::Message(format!("Can't write the zip: {error}")))?;
            size += read as u64;
            done_bytes += read as u64;
            sink.progress(
                done_bytes,
                total_bytes.max(done_bytes),
                index as u64,
                total_files,
            );
            if sink.cancelled() {
                return Err(Failure::Cancelled);
            }
        }
        written.manifest.insert(
            source.rel.clone(),
            EntryInfo {
                size,
                crc: hasher.finalize(),
            },
        );
        sink.progress(
            done_bytes,
            total_bytes.max(done_bytes),
            index as u64 + 1,
            total_files,
        );
    }
    for dir in empty_dirs {
        let name = format!("{}/", dir.trim_end_matches('/'));
        zip.add_directory(name.as_str(), options.last_modified_time(dos_time(None)))
            .map_err(zip_error)?;
        written.dirs.push(name);
    }
    for (name, data) in buffers {
        zip.start_file(name.as_str(), options.last_modified_time(dos_time(None)))
            .map_err(zip_error)?;
        zip.write_all(data)
            .map_err(|error| Failure::Message(format!("Can't write the zip: {error}")))?;
        written.manifest.insert(
            name.clone(),
            EntryInfo {
                size: data.len() as u64,
                crc: crc32fast::hash(data),
            },
        );
        done_bytes += data.len() as u64;
    }
    sink.progress(
        done_bytes,
        total_bytes.max(done_bytes),
        total_files,
        total_files,
    );
    written.total_bytes = done_bytes;
    let out = zip.finish().map_err(zip_error)?;
    let file = out
        .into_inner()
        .map_err(|error| Failure::Message(format!("Can't write the zip: {}", error.error())))?;
    file.sync_all()
        .map_err(|error| Failure::Message(format!("Can't write the zip: {error}")))?;
    Ok(written)
}

/// Reads every entry of the zip at `path` (each one is checked against its
/// CRC-32), and when `expected` is given, that the zip holds exactly those
/// files with the same content.
pub(crate) fn verify_zip(
    path: &Path,
    expected: Option<&Written>,
    sink: &dyn Sink,
) -> Result<ZipCheck, String> {
    let file = File::open(path).map_err(|error| format!("Can't open the zip: {error}"))?;
    let mut archive = ZipArchive::new(BufReader::with_capacity(BUFFER, file))
        .map_err(|error| format!("Not a valid zip: {error}"))?;
    let expected_total = expected.map(|written| written.total_bytes).unwrap_or(0);
    let mut check = ZipCheck::default();
    let mut seen = std::collections::HashSet::new();
    let mut buffer = vec![0u8; BUFFER];
    for index in 0..archive.len() {
        if sink.cancelled() {
            return Err(super::CANCELLED.into());
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Can't read the zip: {error}"))?;
        let name = entry.name().to_string();
        if !seen.insert(name.clone()) {
            return Err(format!("\"{name}\" is in the zip twice"));
        }
        if entry.is_dir() {
            check.dirs += 1;
            continue;
        }
        if entry.encrypted() {
            return Err(format!("\"{name}\" is encrypted"));
        }
        check.files += 1;
        let mut hasher = crc32fast::Hasher::new();
        let mut size = 0u64;
        loop {
            let read = entry
                .read(&mut buffer)
                .map_err(|error| format!("\"{name}\" can't be read back: {error}"))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            size += read as u64;
            check.bytes += read as u64;
            sink.progress(
                check.bytes,
                expected_total.max(check.bytes),
                check.files as u64,
                0,
            );
        }
        let crc = hasher.finalize();
        if size != entry.size() {
            return Err(format!("\"{name}\" has the wrong size in the zip"));
        }
        if crc != entry.crc32() {
            return Err(format!("\"{name}\" has the wrong CRC-32 in the zip"));
        }
        if let Some(expected) = expected {
            match expected.manifest.get(&name) {
                None => return Err(format!("\"{name}\" is in the zip but was never added")),
                Some(info) if info.size != size || info.crc != crc => {
                    return Err(format!(
                        "\"{name}\" in the zip differs from the file that was read"
                    ));
                }
                Some(_) => {}
            }
        }
    }
    if let Some(expected) = expected {
        if check.files != expected.manifest.len() {
            return Err(format!(
                "The zip holds {} files instead of {}",
                check.files,
                expected.manifest.len()
            ));
        }
        if let Some(dir) = expected.dirs.iter().find(|dir| !seen.contains(*dir)) {
            return Err(format!("The folder \"{dir}\" is missing from the zip"));
        }
    }
    Ok(check)
}

/// Size and SHA-256 of a file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Content {
    pub size: u64,
    pub sha256: [u8; 32],
}

pub(crate) fn hash_file(path: &Path) -> io::Result<Content> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; BUFFER];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok(Content {
        size,
        sha256: hasher.finalize().into(),
    })
}

/// Copies `from` to `to` (which must not exist) and flushes it to disk.
/// Returns the size and SHA-256 of what was read, so the zip is not read
/// once more just to hash it.
fn copy_durable(from: &Path, to: &Path, sink: &dyn Sink) -> Result<Content, Failure> {
    let mut input = File::open(from)
        .map_err(|error| Failure::Message(format!("Can't read the zip: {error}")))?;
    let total = input.metadata().map(|meta| meta.len()).unwrap_or(0);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|error| io_failure(format!("Can't create \"{}\"", to.display()), error, to))?;
    let result = (|| {
        let mut buffer = vec![0u8; BUFFER];
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        loop {
            if sink.cancelled() {
                return Err(Failure::Cancelled);
            }
            let read = input
                .read(&mut buffer)
                .map_err(|error| Failure::Message(format!("Can't read the zip: {error}")))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read]).map_err(|error| {
                io_failure(format!("Can't write \"{}\"", to.display()), error, to)
            })?;
            done += read as u64;
            sink.progress(done, total.max(done), 0, 0);
        }
        // Not every virtual file system supports flushing; the copy is
        // verified by reading it back either way.
        let _ = output.sync_all();
        Ok(Content {
            size: done,
            sha256: hasher.finalize().into(),
        })
    })();
    drop(output);
    if result.is_err() {
        let _ = fs::remove_file(to);
    }
    result
}

/// Sleeps `ms`, waking early when cancelled.
fn pause(ms: u64, sink: &dyn Sink) -> Result<(), Failure> {
    let until = std::time::Instant::now() + Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        if sink.cancelled() {
            return Err(Failure::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(50).min(until - std::time::Instant::now()));
    }
    Ok(())
}

/// Reads `path` until it shows `expected`, waiting `delays` in between: a
/// cloud folder may show an older or half-written view for a while.
/// `read` gives the size (and the SHA-256 unless `size_only`).
pub(crate) fn wait_for_content(
    path: &Path,
    expected: Content,
    delays: &[u64],
    size_only: bool,
    read: &mut dyn FnMut(&Path, bool) -> io::Result<Content>,
    sink: &dyn Sink,
) -> Result<usize, Failure> {
    let mut last = String::new();
    let mut waited = 0;
    for attempt in 0..=delays.len() {
        if attempt > 0 {
            pause(delays[attempt - 1], sink)?;
            waited += delays[attempt - 1];
        }
        match read(path, size_only) {
            Ok(actual) if actual.size != expected.size => {
                last = format!(
                    "the copy shows {} instead of {}",
                    format_size(actual.size),
                    format_size(expected.size)
                );
            }
            Ok(actual) if !size_only && actual.sha256 != expected.sha256 => {
                last = format!(
                    "the copy ({}) differs from the verified zip",
                    format_size(expected.size)
                );
            }
            Ok(_) => return Ok(attempt + 1),
            Err(error) => last = error.to_string(),
        }
    }
    Err(Failure::Message(format!(
        "{last} ({} tries in about {} s)",
        delays.len() + 1,
        waited.div_ceil(1000)
    )))
}

fn read_content(path: &Path, size_only: bool) -> io::Result<Content> {
    if size_only {
        return fs::metadata(path).map(|meta| Content {
            size: meta.len(),
            sha256: [0; 32],
        });
    }
    let size = fs::metadata(path)?.len();
    let content = hash_file(path)?;
    if content.size != size {
        return Ok(Content {
            size: content.size,
            sha256: [0; 32],
        });
    }
    Ok(content)
}

pub(crate) fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// The message of a backup that someone else saved under the same name
/// while this one ran.
pub(crate) const NAME_TAKEN: &str = "NAME_TAKEN";

/// Renames `from` to `to`, never replacing an existing `to`: backup_projects
/// may have saved a backup with the same name meanwhile.
fn rename_with_retries(from: &Path, to: &Path, sink: &dyn Sink) -> Result<(), Failure> {
    let mut last = None;
    for attempt in 0..6 {
        if attempt > 0 {
            pause(500, sink)?;
        }
        match rename_no_replace(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(Failure::Coded(
                    NAME_TAKEN,
                    format!(
                        "Another program saved \"{}\" while this backup ran. Nothing was overwritten; back up again to save it as the next version.",
                        to.file_name().unwrap_or_default().to_string_lossy()
                    ),
                ));
            }
            Err(error) => last = Some(error),
        }
    }
    Err(Failure::Message(format!(
        "Can't rename the finished zip: {}",
        last.map(|error| error.to_string()).unwrap_or_default()
    )))
}

#[cfg(windows)]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let wide =
        |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (from_w, to_w) = (wide(from), wide(to));
    // SAFETY: both strings are NUL-terminated and outlive the call. Without
    // MOVEFILE_REPLACE_EXISTING the call fails when `to` exists.
    let ok = unsafe { MoveFileExW(from_w.as_ptr(), to_w.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // ERROR_FILE_EXISTS (80) and ERROR_ALREADY_EXISTS (183).
    if matches!(error.raw_os_error(), Some(80 | 183)) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, error));
    }
    Err(error)
}

#[cfg(not(windows))]
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    if to.exists() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    fs::rename(from, to)
}

pub(crate) fn partial_name(base: &str) -> String {
    let millis = naming::unix_millis(SystemTime::now());
    format!("{PARTIAL_PREFIX}{base}__{millis}.zip")
}

#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the other
    // outputs are optional.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

#[cfg(not(windows))]
fn free_space(_dir: &Path) -> Option<u64> {
    None
}

/// Removes zips left in the staging folder by an interrupted run.
pub(crate) fn clean_staging(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".zip") && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Test hooks: called after the copy at the destination was written.
pub(crate) type AfterCopy<'a> = Option<&'a dyn Fn(&Path)>;

/// Everything that goes into one zip.
pub(crate) struct ZipJob<'a> {
    pub files: &'a [SourceFile],
    pub empty_dirs: &'a [String],
    pub buffers: &'a [(String, Vec<u8>)],
}

/// Creates `final_path` (which must not exist) as a verified zip.
/// `before_finalize` runs once the zip is known to be whole and may still
/// refuse it (the completeness check).
pub(crate) fn write_verified(
    final_path: &Path,
    job: ZipJob,
    staging: Option<&Path>,
    delays: &[u64],
    before_finalize: &mut dyn FnMut(&Written) -> Result<(), Failure>,
    sink: &dyn Sink,
    after_copy: AfterCopy,
) -> Result<Verified, Failure> {
    let dir = final_path
        .parent()
        .ok_or_else(|| Failure::Message("Invalid backup path".into()))?;
    fs::create_dir_all(dir)
        .map_err(|error| io_failure(format!("Can't create \"{}\"", dir.display()), error, dir))?;
    if final_path.exists() {
        return Err(Failure::Message(format!(
            "\"{}\" already exists",
            final_path.display()
        )));
    }
    let base = final_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = dir.join(partial_name(&base));
    let estimate: u64 = job.files.iter().map(|file| file.size).sum::<u64>()
        + job
            .buffers
            .iter()
            .map(|(_, data)| data.len() as u64)
            .sum::<u64>();

    let local = staging.and_then(|staging| {
        fs::create_dir_all(staging).ok()?;
        let needed = estimate + estimate / 20 + STAGING_MARGIN;
        if free_space(staging).is_some_and(|free| free < needed) {
            return None; // build it at the destination instead
        }
        Some(staging.join(format!("{}.zip", uuid::Uuid::new_v4())))
    });

    let result = (|| -> Result<(Written, Content), Failure> {
        if let Some(local) = &local {
            sink.stage(Stage::Zipping);
            let written = write_zip(local, job.files, job.empty_dirs, job.buffers, sink)
                .map_err(|failure| failure.context("Zipping failed"))?;
            sink.stage(Stage::Verifying);
            verify_zip(local, Some(&written), sink)
                .map_err(|error| Failure::from(error).context("The zip failed its check"))?;
            before_finalize(&written)?;
            sink.stage(Stage::Copying);
            let content = copy_durable(local, &partial, sink)
                .map_err(|failure| failure.context("Copying to the backup folder failed"))?;
            if let Some(hook) = after_copy {
                hook(&partial);
            }
            sink.stage(Stage::VerifyingCopy);
            wait_for_content(&partial, content, delays, false, &mut read_content, sink).map_err(
                |failure| failure.context("The copy in the backup folder failed its check"),
            )?;
            Ok((written, content))
        } else {
            sink.stage(Stage::Zipping);
            let written = write_zip(&partial, job.files, job.empty_dirs, job.buffers, sink)
                .map_err(|failure| failure.context("Zipping failed"))?;
            if let Some(hook) = after_copy {
                hook(&partial);
            }
            sink.stage(Stage::Verifying);
            let mut last = String::new();
            let mut ok = false;
            for attempt in 0..=delays.len() {
                if attempt > 0 {
                    pause(delays[attempt - 1], sink)?;
                }
                match verify_zip(&partial, Some(&written), sink) {
                    Ok(_) => {
                        ok = true;
                        break;
                    }
                    Err(error) if error == super::CANCELLED => return Err(Failure::Cancelled),
                    Err(error) => last = error,
                }
            }
            if !ok {
                return Err(Failure::Message(format!(
                    "The zip failed its check: {last}"
                )));
            }
            let content = hash_file(&partial)
                .map_err(|error| Failure::Message(format!("Can't read the zip: {error}")))?;
            before_finalize(&written)?;
            Ok((written, content))
        }
    })();

    let finished = result.and_then(|(written, content)| {
        sink.stage(Stage::Finishing);
        rename_with_retries(&partial, final_path, sink)?;
        Ok((written, content))
    });
    if let Some(local) = &local {
        let _ = fs::remove_file(local);
    }
    let (_written, content) = match finished {
        Ok(done) => done,
        Err(failure) => {
            let _ = fs::remove_file(&partial);
            return Err(failure);
        }
    };
    // The renamed file should show its full size (metadata can lag too).
    let final_check_ok = wait_for_content(
        final_path,
        content,
        &delays[..delays.len().min(6)],
        true,
        &mut read_content,
        sink,
    )
    .is_ok();
    Ok(Verified {
        zip_size: content.size,
        final_check_ok,
    })
}

/// Whether the zip at `path` can be opened.
#[cfg(test)]
pub(crate) fn zip_is_readable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.len() >= MIN_ZIP_SIZE)
        && File::open(path)
            .ok()
            .and_then(|file| ZipArchive::new(BufReader::new(file)).ok())
            .is_some()
}

pub(crate) fn staging_dir() -> Option<PathBuf> {
    crate::storage::local_dir()
        .ok()
        .map(|dir| dir.join("project-backups-staging"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub(crate) struct Quiet;
    impl Sink for Quiet {
        fn stage(&self, _stage: Stage) {}
        fn progress(&self, _: u64, _: u64, _: u64, _: u64) {}
        fn cancelled(&self) -> bool {
            false
        }
    }

    pub(crate) fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myle-archive-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    pub(crate) fn source(root: &Path, rel: &str, body: &[u8]) -> SourceFile {
        let abs = root.join(rel);
        fs::create_dir_all(abs.parent().unwrap()).unwrap();
        fs::write(&abs, body).unwrap();
        SourceFile {
            rel: rel.into(),
            abs,
            size: body.len() as u64,
            modified: Some(SystemTime::now()),
        }
    }

    #[test]
    fn a_written_zip_reads_back_whole() {
        let root = temp("write");
        let files = vec![
            source(&root, "src/main.rs", b"fn main() {}"),
            source(&root, "Ελληνικά/σημειώσεις.txt", "γεια".as_bytes()),
            source(&root, "empty.txt", b""),
        ];
        let buffers = vec![(".backup-info.json".to_string(), b"{}".to_vec())];
        let zip = root.join("out.zip");
        let written = write_zip(&zip, &files, &["assets/empty".into()], &buffers, &Quiet).unwrap();
        assert_eq!(written.manifest.len(), 4);
        assert_eq!(written.dirs, ["assets/empty/"]);
        let check = verify_zip(&zip, Some(&written), &Quiet).unwrap();
        assert_eq!((check.files, check.dirs), (4, 1));
        // A different source no longer matches.
        let mut other = Written::default();
        other
            .manifest
            .insert("src/main.rs".into(), EntryInfo { size: 1, crc: 0 });
        assert!(verify_zip(&zip, Some(&other), &Quiet).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_and_partial_zips_are_refused() {
        let root = temp("corrupt");
        let files = vec![source(&root, "a.txt", &b"hello world ".repeat(5000))];
        let zip = root.join("good.zip");
        let written = write_zip(&zip, &files, &[], &[], &Quiet).unwrap();
        let bytes = fs::read(&zip).unwrap();
        // Cut short, as a copy still being uploaded.
        let cut = root.join("cut.zip");
        fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        assert!(verify_zip(&cut, Some(&written), &Quiet).is_err());
        assert!(!zip_is_readable(&cut));
        // A flipped byte inside the compressed data.
        let mut flipped = bytes.clone();
        flipped[60] ^= 0xff;
        let bad = root.join("bad.zip");
        fs::write(&bad, &flipped).unwrap();
        assert!(verify_zip(&bad, Some(&written), &Quiet).is_err());
        // Too small to be a zip.
        let tiny = root.join("tiny.zip");
        fs::write(&tiny, b"PK").unwrap();
        assert!(!zip_is_readable(&tiny));
        assert!(zip_is_readable(&zip));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_stale_view_is_read_again_until_it_matches() {
        let root = temp("stale");
        let path = root.join("x.zip");
        fs::write(&path, b"final content").unwrap();
        let expected = hash_file(&path).unwrap();
        let calls = Cell::new(0);
        let mut read = |path: &Path, size_only: bool| {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => Ok(Content {
                    size: 3,
                    sha256: [0; 32],
                }), // still uploading
                2 => Ok(Content {
                    size: expected.size,
                    sha256: [1; 32],
                }), // old content
                3 => Err(io::Error::other("busy")),
                _ => read_content(path, size_only),
            }
        };
        let attempts =
            wait_for_content(&path, expected, &[1, 1, 1, 1], false, &mut read, &Quiet).unwrap();
        assert_eq!(attempts, 4);
        let mut never = |_: &Path, _: bool| {
            Ok(Content {
                size: 1,
                sha256: [0; 32],
            })
        };
        let error =
            wait_for_content(&path, expected, &[1, 1], false, &mut never, &Quiet).unwrap_err();
        assert!(matches!(error, Failure::Message(text) if text.contains("3 tries")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_verified_zip_is_staged_copied_and_renamed() {
        let root = temp("verified");
        let files = vec![
            source(&root, "src/a.txt", b"a"),
            source(&root, "src/b.txt", b"b"),
        ];
        let month = root
            .join("Projects Backup")
            .join("App")
            .join("2026-10 Οκτώβριος");
        let final_path = month.join("App_D8_V1.zip");
        let staging = root.join("staging");
        let mut checked = false;
        let verified = write_verified(
            &final_path,
            ZipJob {
                files: &files,
                empty_dirs: &[],
                buffers: &[],
            },
            Some(&staging),
            &[1, 1],
            &mut |written| {
                checked = written.manifest.len() == 2;
                Ok(())
            },
            &Quiet,
            None,
        )
        .unwrap();
        assert!(checked && verified.final_check_ok);
        assert!(zip_is_readable(&final_path));
        assert_eq!(
            fs::read_dir(&month).unwrap().count(),
            1,
            "no partial left behind"
        );
        assert_eq!(
            fs::read_dir(&staging).unwrap().count(),
            0,
            "staging cleaned"
        );
        // The same name is never overwritten.
        let again = write_verified(
            &final_path,
            ZipJob {
                files: &files,
                empty_dirs: &[],
                buffers: &[],
            },
            Some(&staging),
            &[1],
            &mut |_| Ok(()),
            &Quiet,
            None,
        );
        assert!(again.is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_copy_that_never_settles_is_removed_and_not_named() {
        let root = temp("unsettled");
        let files = vec![source(&root, "a.txt", &b"content ".repeat(1000))];
        let month = root.join("dest");
        let final_path = month.join("App_D8_V2.zip");
        // The destination keeps showing a cut-off file.
        let truncate = |partial: &Path| {
            let bytes = fs::read(partial).unwrap();
            fs::write(partial, &bytes[..bytes.len() - 10]).unwrap();
        };
        let error = write_verified(
            &final_path,
            ZipJob {
                files: &files,
                empty_dirs: &[],
                buffers: &[],
            },
            Some(&root.join("staging")),
            &[1, 1],
            &mut |_| Ok(()),
            &Quiet,
            Some(&truncate),
        )
        .unwrap_err();
        assert!(matches!(error, Failure::Message(text) if text.contains("failed its check")));
        assert!(!final_path.exists());
        assert_eq!(fs::read_dir(&month).unwrap().count(), 0, "partial removed");

        // Without staging, a zip broken at the destination is refused too.
        let error = write_verified(
            &final_path,
            ZipJob {
                files: &files,
                empty_dirs: &[],
                buffers: &[],
            },
            None,
            &[1],
            &mut |_| Ok(()),
            &Quiet,
            Some(&truncate),
        );
        assert!(error.is_err());
        assert!(!final_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_completeness_check_can_still_refuse_the_zip() {
        let root = temp("refuse");
        let files = vec![source(&root, "a.txt", b"a")];
        let final_path = root.join("dest").join("App_D8_V3.zip");
        let error = write_verified(
            &final_path,
            ZipJob {
                files: &files,
                empty_dirs: &[],
                buffers: &[],
            },
            Some(&root.join("staging")),
            &[1],
            &mut |_| Err(Failure::Message("src/missing.rs is not in the zip".into())),
            &Quiet,
            None,
        )
        .unwrap_err();
        assert!(matches!(error, Failure::Message(text) if text.contains("missing.rs")));
        assert!(!final_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancelling_removes_the_half_written_zip() {
        struct Cancel(AtomicBool);
        impl Sink for Cancel {
            fn stage(&self, _stage: Stage) {}
            fn progress(&self, _: u64, _: u64, _: u64, _: u64) {
                self.0.store(true, Ordering::Relaxed);
            }
            fn cancelled(&self) -> bool {
                self.0.load(Ordering::Relaxed)
            }
        }
        let root = temp("cancel");
        let files = vec![source(&root, "a.txt", b"a"), source(&root, "b.txt", b"b")];
        let zip = root.join("x.zip");
        let error = write_zip(&zip, &files, &[], &[], &Cancel(AtomicBool::new(false))).unwrap_err();
        assert!(matches!(error, Failure::Cancelled));
        assert!(!zip.exists());
        let _ = fs::remove_dir_all(root);
    }
}
