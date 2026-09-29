// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows file-system primitives for the single-file transaction (SAFETY_MODEL §4.1).
//!
//! Behaviour of every primitive here was measured in S2 (docs/SPIKE_REPORT.md §3) on local
//! NTFS and SMB (loopback). The transaction logic itself lives in higher layers.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_ID_INFO, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo, FileIdInfo, FlushFileBuffers,
    GetDiskFreeSpaceExW, GetDriveTypeW, GetFileInformationByHandle, GetFileInformationByHandleEx,
    GetShortPathNameW, GetVolumePathNameW, MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
};

const FILE_READ_ATTRIBUTES: u32 = 0x80;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;

/// A Win32 error code returned by a primitive (e.g. 32 sharing violation, 5 access denied,
/// 1176 ERROR_UNABLE_TO_MOVE_REPLACEMENT, 1177 ERROR_UNABLE_TO_MOVE_REPLACEMENT_2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Win32Error(pub u32);

impl std::fmt::Display for Win32Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Win32 error {}", self.0)
    }
}
impl std::error::Error for Win32Error {}

fn last_error() -> Win32Error {
    // SAFETY: GetLastError has no preconditions.
    Win32Error(unsafe { GetLastError() })
}

/// NUL-terminated wide path with the verbatim prefix (`\\?\` or `\\?\UNC\`) for long paths.
/// `p` must be absolute.
pub fn wide(p: &Path) -> Vec<u16> {
    let s = p.as_os_str().to_string_lossy().replace('/', "\\");
    let s = if s.starts_with(r"\\?\") {
        s
    } else if let Some(rest) = s.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")
    } else {
        format!(r"\\?\{s}")
    };
    std::ffi::OsStr::new(&s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Whether every character of `p` has an exact form in the system ANSI code page. A program that
/// learns its own location through the ANSI API (the ExifTool launcher, Perl) sees `?` in place of
/// the others and cannot find its files (RESEARCH_NOTES F-105). No best-fit mapping: `Ł` → `L`
/// would name another folder.
pub fn ansi_exact(p: &Path) -> bool {
    use windows_sys::Win32::Globalization::WideCharToMultiByte;
    const CP_ACP: u32 = 0;
    const WC_NO_BEST_FIT_CHARS: u32 = 0x400;
    let w: Vec<u16> = p.as_os_str().encode_wide().collect();
    if w.is_empty() {
        return true;
    }
    let mut used_default = 0;
    // SAFETY: `w` is valid for its length; a null output buffer of size 0 only measures.
    let n = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            WC_NO_BEST_FIT_CHARS,
            w.as_ptr(),
            w.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            &mut used_default,
        )
    };
    n > 0 && used_default == 0
}

/// The 8.3 short form of an existing path, without the verbatim prefix. Components the volume
/// keeps no short name for stay long.
pub fn short_path(p: &Path) -> io::Result<PathBuf> {
    let w = wide(p);
    let mut buf = vec![0u16; 32_768];
    // SAFETY: `w` is NUL-terminated; `buf` is writable for the length passed.
    let n = unsafe { GetShortPathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 || n as usize > buf.len() {
        return Err(io::Error::last_os_error());
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    Ok(PathBuf::from(match s.strip_prefix(r"\\?\UNC\") {
        Some(r) => format!(r"\\{r}"),
        None => s.strip_prefix(r"\\?\").unwrap_or(&s).to_owned(),
    }))
}

/// Volume serial number + 128-bit file ID (stable identity across renames on the same volume).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    pub volume: u64,
    pub id: [u8; 16],
}

pub fn file_id(f: &File) -> io::Result<FileId> {
    // SAFETY: `info` is a properly sized, writable FILE_ID_INFO; the handle is valid for `f`'s lifetime.
    unsafe {
        let mut info: FILE_ID_INFO = std::mem::zeroed();
        if GetFileInformationByHandleEx(
            f.as_raw_handle() as HANDLE,
            FileIdInfo,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(FileId {
            volume: info.VolumeSerialNumber,
            id: info.FileId.Identifier,
        })
    }
}

pub fn link_count(f: &File) -> io::Result<u32> {
    // SAFETY: `info` is writable and correctly sized; the handle is valid.
    unsafe {
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        if GetFileInformationByHandle(f.as_raw_handle() as HANDLE, &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info.nNumberOfLinks)
    }
}

/// Attribute-only open that does not follow reparse points and never conflicts with other openers.
pub fn open_attr(p: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)
}

pub fn file_id_of_path(p: &Path) -> io::Result<FileId> {
    file_id(&open_attr(p)?)
}

/// The transaction lock: read access; deny write sharing; allow read and delete/rename sharing.
/// ExifTool's own read opens (GENERIC_READ, share READ|WRITE) and `ReplaceFileW` both work while
/// it is held; other writers get ERROR_SHARING_VIOLATION (S2).
pub fn open_lock(p: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(GENERIC_READ)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(p)
}

/// Reparse tag of the file behind `f`; 0 when it is not a reparse point.
pub fn reparse_tag(f: &File) -> io::Result<u32> {
    // SAFETY: `info` is a properly sized, writable FILE_ATTRIBUTE_TAG_INFO; the handle is valid.
    unsafe {
        let mut info: FILE_ATTRIBUTE_TAG_INFO = std::mem::zeroed();
        if GetFileInformationByHandleEx(
            f.as_raw_handle() as HANDLE,
            FileAttributeTagInfo,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            info.ReparseTag
        } else {
            0
        })
    }
}

/// What kind of reparse point a path is (SAFETY_MODEL §8.3, §8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reparse {
    None,
    /// A name surrogate: symbolic link, junction and the like, pointing somewhere else.
    Link,
    /// A file of a cloud sync client (OneDrive and others using the Cloud Files API), downloaded
    /// or not.
    Cloud,
    /// Any other kind (deduplication, compression overlays…).
    Other,
}

impl Reparse {
    pub fn of_tag(tag: u32) -> Reparse {
        const NAME_SURROGATE: u32 = 0x2000_0000;
        const CLOUD: u32 = 0x9000_001A; // IO_REPARSE_TAG_CLOUD; CLOUD_1..F set bits 12..15
        match tag {
            0 => Reparse::None,
            t if t & 0xFFFF_0FFF == CLOUD => Reparse::Cloud,
            t if t & NAME_SURROGATE != 0 => Reparse::Link,
            _ => Reparse::Other,
        }
    }
}

/// Properties relevant to the pre-check (SAFETY_MODEL §4.1 step 2, §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub read_only: bool,
    pub reparse_point: bool,
    pub reparse: Reparse,
    pub cloud_placeholder: bool,
    pub links: u32,
    pub size: u64,
}

impl Probe {
    /// Why this file is not written in place, if it is not (SAFETY_MODEL §8.1, §8.3, §8.6, §8.7).
    pub fn refusal(&self) -> Option<&'static str> {
        Some(if self.cloud_placeholder {
            "cloud placeholder that is not downloaded"
        } else if self.reparse == Reparse::Cloud {
            "cloud file of a sync client: writing through the sync client is not verified yet"
        } else if self.reparse == Reparse::Link {
            "symbolic link or junction (not written)"
        } else if self.reparse_point {
            "reparse point (not written)"
        } else if self.links > 1 {
            "file has more than one hard link (not written)"
        } else if self.read_only {
            "read-only attribute is set (treated as locked by the user)"
        } else {
            return None;
        })
    }
}

pub fn probe(p: &Path) -> io::Result<Probe> {
    let f = open_attr(p)?;
    let m = f.metadata()?;
    let a = m.file_attributes();
    Ok(Probe {
        read_only: a & FILE_ATTRIBUTE_READONLY != 0,
        reparse_point: a & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        reparse: Reparse::of_tag(reparse_tag(&f)?),
        cloud_placeholder: a & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE) != 0,
        links: link_count(&f)?,
        size: m.len(),
    })
}

/// `ReplaceFileW(replaced, replacement, backup, 0)`.
///
/// Not atomic with respect to process termination: the original content can be left under
/// `backup` with `replaced` missing (S2, 14 of 300 random kills). The backup name must therefore
/// be recorded durably beforehand, and must not exist: an existing file at `backup` is
/// overwritten without error (S2 F5). Use [`ensure_absent`] right before calling this.
pub fn replace_file(replaced: &Path, replacement: &Path, backup: &Path) -> Result<(), Win32Error> {
    let (a, b, c) = (wide(replaced), wide(replacement), wide(backup));
    // SAFETY: all three are valid NUL-terminated wide strings; the reserved pointers are null.
    let ok = unsafe {
        ReplaceFileW(
            a.as_ptr(),
            b.as_ptr(),
            c.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 { Err(last_error()) } else { Ok(()) }
}

/// Rename that never replaces an existing target (MOVEFILE_WRITE_THROUGH, no REPLACE_EXISTING).
pub fn move_no_replace(src: &Path, dst: &Path) -> Result<(), Win32Error> {
    let (a, b) = (wide(src), wide(dst));
    // SAFETY: valid NUL-terminated wide strings.
    let ok = unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 { Err(last_error()) } else { Ok(()) }
}

/// Error if anything (file, directory, link) exists at `p`.
pub fn ensure_absent(p: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(p) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", p.display()),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn flush(f: &File) -> io::Result<()> {
    // SAFETY: valid handle opened with write access.
    if unsafe { FlushFileBuffers(f.as_raw_handle() as HANDLE) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn flush_path(p: &Path) -> io::Result<()> {
    let f = OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(p)?;
    flush(&f)
}

pub type Hash = [u8; 32];

pub fn hex(h: &Hash) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// BLAKE3 of everything readable from `r`.
pub fn hash_reader(mut r: impl Read) -> io::Result<Hash> {
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(*h.finalize().as_bytes());
        }
        h.update(&buf[..n]);
    }
}

pub fn hash_path(p: &Path) -> io::Result<Hash> {
    let f = OpenOptions::new()
        .access_mode(GENERIC_READ)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(p)?;
    hash_reader(f)
}

/// Stream `src` (e.g. the lock handle, positioned at 0) into a new file `dst` (never overwrites),
/// hashing on the fly, then flush `dst`. On error the partial `dst` is removed.
pub fn copy_new_hashing(src: &mut File, dst: &Path) -> io::Result<Hash> {
    let mut out = OpenOptions::new().write(true).create_new(true).open(dst)?;
    let r = (|| {
        let mut h = blake3::Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        flush(&out)?;
        Ok(*h.finalize().as_bytes())
    })();
    if r.is_err() {
        drop(out);
        let _ = std::fs::remove_file(dst);
    }
    r
}

/// Win32 112 ERROR_DISK_FULL or 39 ERROR_HANDLE_DISK_FULL.
pub fn is_disk_full(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(112 | 39)) || e.kind() == io::ErrorKind::StorageFull
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeSpace {
    /// Bytes available to this process (respects quotas).
    pub free: u64,
    pub total: u64,
}

/// Free and total space of the volume that holds directory `dir`.
pub fn volume_space(dir: &Path) -> io::Result<VolumeSpace> {
    let mut w = wide(dir);
    w.pop(); // NUL
    if w.last() != Some(&(b'\\' as u16)) {
        w.push(b'\\' as u16);
    }
    w.push(0);
    let (mut free, mut total, mut all_free) = (0u64, 0u64, 0u64);
    // SAFETY: `w` is a NUL-terminated wide string; the out pointers are valid u64s.
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut free, &mut total, &mut all_free) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(VolumeSpace { free, total })
}

/// The mount point of the volume holding `p` (`C:\`, `\\server\share\`, or a mounted folder),
/// as returned by GetVolumePathNameW with the verbatim prefix removed.
pub fn volume_root(p: &Path) -> io::Result<PathBuf> {
    let w = wide(p);
    let mut buf = vec![0u16; 32_768];
    // SAFETY: `w` is NUL-terminated; `buf` is writable for the length passed.
    let ok = unsafe { GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..n]);
    let s = if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{r}")
    } else if let Some(r) = s.strip_prefix(r"\\?\") {
        r.to_owned()
    } else {
        s
    };
    Ok(PathBuf::from(s))
}

/// Storage class of a volume, for per-volume IO concurrency (ARCHITECTURE §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    Ssd,
    Hdd,
    Network,
    Removable,
    /// Could not be determined: treated like an HDD.
    Unknown,
}

impl VolumeKind {
    /// Concurrent file transactions allowed on one volume of this kind.
    pub fn io_limit(self) -> usize {
        match self {
            VolumeKind::Ssd => 4,
            VolumeKind::Network => 2,
            VolumeKind::Hdd | VolumeKind::Removable | VolumeKind::Unknown => 1,
        }
    }
}

/// Classify the volume whose mount point is `root` (from [`volume_root`]). A local fixed disk is
/// an SSD only if the storage driver reports no seek penalty.
pub fn volume_kind(root: &Path) -> VolumeKind {
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    let mut w = wide(root);
    w.pop();
    if w.last() != Some(&(b'\\' as u16)) {
        w.push(b'\\' as u16);
    }
    w.push(0);
    // SAFETY: NUL-terminated wide string.
    match unsafe { GetDriveTypeW(w.as_ptr()) } {
        DRIVE_REMOTE => VolumeKind::Network,
        DRIVE_REMOVABLE => VolumeKind::Removable,
        DRIVE_FIXED => match seek_penalty(root) {
            Some(false) => VolumeKind::Ssd,
            Some(true) => VolumeKind::Hdd,
            None => VolumeKind::Unknown,
        },
        _ => VolumeKind::Unknown,
    }
}

/// IOCTL_STORAGE_QUERY_PROPERTY(StorageDeviceSeekPenaltyProperty) on `\\.\X:` (no access rights
/// needed). None when the volume has no drive letter or the driver does not answer.
fn seek_penalty(root: &Path) -> Option<bool> {
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{
        DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery,
        STORAGE_PROPERTY_QUERY, StorageDeviceSeekPenaltyProperty,
    };
    let s = root.to_string_lossy();
    let letter = s.chars().next().filter(|c| c.is_ascii_alphabetic())?;
    if !s[1..].starts_with(':') {
        return None;
    }
    let dev: Vec<u16> = format!(r"\\.\{letter}:")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: plain Win32 calls; the handle is closed before returning; buffers outlive the call.
    unsafe {
        let h = CreateFileW(
            dev.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return None;
        }
        let query = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceSeekPenaltyProperty,
            QueryType: PropertyStandardQuery,
            AdditionalParameters: [0],
        };
        let mut out: DEVICE_SEEK_PENALTY_DESCRIPTOR = std::mem::zeroed();
        let mut got = 0u32;
        let ok = DeviceIoControl(
            h,
            IOCTL_STORAGE_QUERY_PROPERTY,
            &query as *const _ as *const _,
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            &mut out as *mut _ as *mut _,
            std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            &mut got,
            std::ptr::null_mut(),
        );
        windows_sys::Win32::Foundation::CloseHandle(h);
        (ok != 0 && got as usize >= std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>())
            .then_some(out.IncursSeekPenalty)
    }
}

/// Largest volume [`fill_volume`] agrees to fill.
pub const FILL_MAX_VOLUME: u64 = 2 << 30;

/// Test lab only (real disk-full tests): allocate every free byte of the volume that holds `dir`
/// in new files named `mm-fill-<token>.bin` inside `dir`, and return them (delete them to free the
/// space again). Refuses volumes larger than [`FILL_MAX_VOLUME`], so it can never be pointed at a
/// user's real disk.
pub fn fill_volume(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let space = volume_space(dir)?;
    if space.total > FILL_MAX_VOLUME {
        return Err(io::Error::other(format!(
            "refusing to fill a volume of {} bytes (limit {FILL_MAX_VOLUME})",
            space.total
        )));
    }
    let mut made = Vec::new();
    // one large allocation, then small writes until the file system reports disk full
    let big = dir.join(format!("mm-fill-{}.bin", random_token()?));
    let f = OpenOptions::new().write(true).create_new(true).open(&big)?;
    made.push(big);
    let mut len = space.free;
    while len > 0 {
        match f.set_len(len) {
            Ok(()) => break,
            Err(_) => len = len.saturating_sub(len / 64 + 65536),
        }
    }
    let small = dir.join(format!("mm-fill-{}.bin", random_token()?));
    let mut g = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&small)?;
    made.push(small);
    let block = [0u8; 4096];
    for _ in 0..(1 << 18) {
        match g.write_all(&block).and_then(|_| g.flush()) {
            Ok(()) => {}
            Err(e) if is_disk_full(&e) => return Ok(made),
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("volume did not fill up"))
}

/// What a folder import found (ARCHITECTURE §6.1), from the directory listing alone: no file is
/// opened, so a cloud placeholder is not downloaded by looking at it.
#[derive(Debug, Default)]
pub struct Walk {
    pub files: Vec<PathBuf>,
    /// Directory symbolic links and junctions: listed, not entered (SAFETY_MODEL §8.6).
    pub not_followed: Vec<PathBuf>,
    /// Cloud files that are not on this computer (SAFETY_MODEL §8.3): not read by default.
    pub placeholders: Vec<PathBuf>,
    /// System and hidden folders (`$RECYCLE.BIN`, `System Volume Information`…): not entered
    /// (PRODUCT_SPEC §6.1).
    pub skipped_folders: Vec<PathBuf>,
    pub errors: Vec<(PathBuf, String)>,
}

/// A folder an import does not enter: marked system or hidden, or one of Windows' own.
fn is_system_folder(path: &Path, attributes: u32) -> bool {
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    attributes & (HIDDEN | SYSTEM) != 0
        || name == "$recycle.bin"
        || name == "system volume information"
}

/// Every file under `root`, depth first, in name order within a folder.
pub fn walk(root: &Path) -> Walk {
    use std::os::windows::fs::MetadataExt;
    let mut w = Walk::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .filter_map(|e| match e {
                    Ok(e) => Some(e),
                    Err(err) => {
                        w.errors.push((dir.clone(), err.to_string()));
                        None
                    }
                })
                .collect(),
            Err(err) => {
                w.errors.push((dir.clone(), err.to_string()));
                continue;
            }
        };
        entries.sort_by_key(|e| e.file_name());
        let mut subdirs = Vec::new();
        for e in entries {
            let path = e.path();
            // from the listing (FindNextFileW), without opening or following the entry
            let a = match e.metadata() {
                Ok(m) => m.file_attributes(),
                Err(err) => {
                    w.errors.push((path, err.to_string()));
                    continue;
                }
            };
            if a & FILE_ATTRIBUTE_DIRECTORY != 0 {
                if a & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    w.not_followed.push(path);
                } else if is_system_folder(&path, a) {
                    w.skipped_folders.push(path);
                } else {
                    subdirs.push(path);
                }
            } else if a & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE) != 0 {
                w.placeholders.push(path);
            } else {
                w.files.push(path);
            }
        }
        stack.extend(subdirs.into_iter().rev());
    }
    w
}

/// Folders a cloud client keeps in sync (SAFETY_MODEL §8.3), best effort: OneDrive (the
/// environment variables its client sets), Dropbox (its `info.json`), iCloud Drive (its default
/// folder). Writing there works, but the client uploads every change and may create conflicted
/// copies when the file changes elsewhere at the same time.
pub fn sync_roots() -> Vec<(PathBuf, &'static str)> {
    let mut roots = Vec::new();
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            let p = PathBuf::from(v);
            if !roots.iter().any(|(r, _): &(PathBuf, &str)| r == &p) {
                roots.push((p, "OneDrive"));
            }
        }
    }
    for base in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(b) = std::env::var_os(base)
            && let Ok(text) =
                std::fs::read_to_string(PathBuf::from(b).join("Dropbox").join("info.json"))
        {
            for p in dropbox_paths(&text) {
                roots.push((p, "Dropbox"));
            }
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let icloud = PathBuf::from(home).join("iCloudDrive");
        if icloud.is_dir() {
            roots.push((icloud, "iCloud Drive"));
        }
    }
    roots
}

/// The `path` of every account in Dropbox's `info.json`.
fn dropbox_paths(info_json: &str) -> Vec<PathBuf> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(info_json) else {
        return vec![];
    };
    v.as_object()
        .map(|o| {
            o.values()
                .filter_map(|acct| acct.get("path")?.as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The sync client whose folder holds `p`: whole path components, letter case ignored.
pub fn sync_provider(p: &Path, roots: &[(PathBuf, &'static str)]) -> Option<&'static str> {
    let lower = |q: &Path| -> Vec<String> {
        q.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let pc = lower(p);
    roots.iter().find_map(|(r, name)| {
        let rc = lower(r);
        (!rc.is_empty() && pc.len() >= rc.len() && pc[..rc.len()] == rc[..]).then_some(*name)
    })
}

/// Whether this process runs with administrator rights (an elevated token). MoriMeta never
/// writes then (SECURITY_MODEL §4.1): a compromised ExifTool would run with the same rights.
pub fn is_elevated() -> io::Result<bool> {
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: plain Win32 calls; the token handle is closed here, `e` is sized for the call.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut e as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ) != 0;
        let err = io::Error::last_os_error();
        windows_sys::Win32::Foundation::CloseHandle(token);
        if !ok {
            return Err(err);
        }
        Ok(e.TokenIsElevated != 0)
    }
}

/// Keeps the system from sleeping while an Operation runs (SAFETY_MODEL §8.14); the display may
/// still turn off. The request belongs to the calling thread and ends when the guard is dropped.
pub struct KeepAwake(());

impl KeepAwake {
    pub fn new() -> io::Result<Self> {
        use windows_sys::Win32::System::Power::{
            ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
        };
        // SAFETY: plain flag call; a zero return means failure.
        if unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) } == 0 {
            return Err(io::Error::other("SetThreadExecutionState failed"));
        }
        Ok(Self(()))
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Power::{ES_CONTINUOUS, SetThreadExecutionState};
        // SAFETY: as above; clears this thread's request.
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    }
}

/// 64 random bits as 16 hex digits (for `.mmtmp-` / `.mmbak-` names).
pub fn random_token() -> io::Result<String> {
    let v = getrandom::u64().map_err(|e| io::Error::other(e.to_string()))?;
    Ok(format!("{v:016x}"))
}

/// `<stem><infix><token><.ext>` next to `p` (same directory, hence same volume).
pub fn sibling_name(p: &Path, infix: &str, token: &str) -> PathBuf {
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    p.with_file_name(format!("{stem}{infix}{token}{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mm-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn walk_lists_without_following_links_or_reading_placeholders() {
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, SetFileAttributesW};
        let d = dir("walk");
        let outside = dir("walk-outside");
        std::fs::write(outside.join("elsewhere.jpg"), b"x").unwrap();
        std::fs::create_dir_all(d.join("b").join("c")).unwrap();
        std::fs::write(d.join("a.jpg"), b"a").unwrap();
        std::fs::create_dir_all(d.join("$RECYCLE.BIN")).unwrap();
        std::fs::write(d.join("$RECYCLE.BIN").join("deleted.jpg"), b"d").unwrap();
        std::fs::write(d.join("b").join("c").join("deep.nef"), b"n").unwrap();
        let cloud = d.join("b").join("cloud.jpg");
        std::fs::write(&cloud, b"c").unwrap();
        // SAFETY: NUL-terminated path; OFFLINE is how a not-downloaded file is marked.
        assert_ne!(
            unsafe { SetFileAttributesW(wide(&cloud).as_ptr(), FILE_ATTRIBUTE_OFFLINE) },
            0
        );
        let junction = d.join("j");
        let ok = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J failed");
        let w = walk(&d);
        let rel = |v: &[PathBuf]| {
            v.iter()
                .map(|p| {
                    p.strip_prefix(&d)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(rel(&w.files), ["a.jpg", "b/c/deep.nef"]);
        assert_eq!(rel(&w.placeholders), ["b/cloud.jpg"]);
        assert_eq!(rel(&w.not_followed), ["j"]);
        assert_eq!(rel(&w.skipped_folders), ["$RECYCLE.BIN"]);
        assert!(w.errors.is_empty(), "{:?}", w.errors);
        // SAFETY: as above.
        unsafe { SetFileAttributesW(wide(&cloud).as_ptr(), FILE_ATTRIBUTE_NORMAL) };
        std::fs::remove_dir(&junction).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn sync_folders_by_whole_components() {
        let roots = vec![
            (PathBuf::from(r"S:\Sync\m\OneDrive"), "OneDrive"),
            (PathBuf::from(r"D:\Dropbox"), "Dropbox"),
        ];
        let at = |s: &str| sync_provider(Path::new(s), &roots);
        assert_eq!(at(r"s:\sync\M\onedrive\Photos\a.jpg"), Some("OneDrive"));
        assert_eq!(at(r"S:\Sync\m\OneDrive"), Some("OneDrive"));
        assert_eq!(at(r"S:\Sync\m\OneDrive - Work\a.jpg"), None);
        assert_eq!(at(r"S:\Sync\m\OneDriveX\a.jpg"), None);
        assert_eq!(at(r"D:\Dropbox\x\y.NEF"), Some("Dropbox"));
        assert_eq!(at(r"E:\Photos\a.jpg"), None);
        let info = r#"{"personal": {"path": "S:\\Sync\\m\\Dropbox", "host": 1},
                       "business": {"path": "S:\\Sync\\m\\Dropbox (Team)"}}"#;
        let mut p = dropbox_paths(info);
        p.sort();
        assert_eq!(
            p,
            [
                PathBuf::from(r"S:\Sync\m\Dropbox"),
                PathBuf::from(r"S:\Sync\m\Dropbox (Team)")
            ]
        );
        assert!(dropbox_paths("not json").is_empty());
    }

    #[test]
    fn elevation_can_be_determined() {
        // the answer depends on how the tests run (CI runners are elevated); it must not fail
        assert!(is_elevated().is_ok());
    }

    #[test]
    fn keep_awake_is_held_until_dropped() {
        use windows_sys::Win32::System::Power::{
            ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
        };
        let g = KeepAwake::new().unwrap();
        // SAFETY: flag calls; each returns this thread's previous state.
        let held = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        assert_ne!(held & ES_SYSTEM_REQUIRED, 0);
        drop(g);
        let after = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        assert_eq!(after & ES_SYSTEM_REQUIRED, 0);
    }

    #[test]
    fn lock_blocks_writers_but_not_replace() {
        let d = dir("lock");
        let orig = d.join("a.jpg");
        std::fs::write(&orig, b"original").unwrap();
        let temp = sibling_name(&orig, ".mmtmp-", &random_token().unwrap());
        std::fs::write(&temp, b"new content").unwrap();
        let bak = sibling_name(&orig, ".mmbak-", &random_token().unwrap());
        let mut lock = open_lock(&orig).unwrap();
        let id = file_id(&lock).unwrap();
        assert_eq!(file_id_of_path(&orig).unwrap(), id);
        let w = OpenOptions::new().write(true).open(&orig);
        assert_eq!(w.unwrap_err().raw_os_error(), Some(32));
        // reading through another handle still works
        assert_eq!(std::fs::read(&orig).unwrap(), b"original");
        ensure_absent(&bak).unwrap();
        replace_file(&orig, &temp, &bak).unwrap();
        assert_eq!(std::fs::read(&orig).unwrap(), b"new content");
        assert_eq!(std::fs::read(&bak).unwrap(), b"original");
        lock.rewind().unwrap();
        assert_eq!(
            hash_reader(&mut lock).unwrap(),
            *blake3::hash(b"original").as_bytes()
        );
        assert_eq!(file_id(&lock).unwrap(), file_id_of_path(&bak).unwrap());
    }

    #[test]
    fn replace_failure_leaves_disk_unchanged() {
        let d = dir("fail");
        let orig = d.join("a.jpg");
        std::fs::write(&orig, b"original").unwrap();
        let temp = d.join("a.tmp.jpg");
        std::fs::write(&temp, b"new").unwrap();
        let bak = d.join("a.bak.jpg");
        let holder = OpenOptions::new()
            .access_mode(GENERIC_READ)
            .share_mode(FILE_SHARE_READ)
            .open(&orig)
            .unwrap();
        assert_eq!(replace_file(&orig, &temp, &bak), Err(Win32Error(32)));
        drop(holder);
        assert_eq!(std::fs::read(&orig).unwrap(), b"original");
        assert!(temp.exists());
        assert!(!bak.exists());
    }

    #[test]
    fn ansi_exact_and_short_path() {
        use windows_sys::Win32::Globalization::GetACP;
        assert!(ansi_exact(Path::new(r"D:\Photos\Mori Morii\Programs")));
        // SAFETY: no preconditions.
        let utf8_acp = unsafe { GetACP() } == 65001;
        if !utf8_acp {
            // no ANSI code page but UTF-8 has an emoji
            assert!(!ansi_exact(Path::new("D:\\Photos\\Morii \u{1F4F7}")));
        }
        let d = std::env::temp_dir().join(format!("mm-short-{}", std::process::id()));
        let far = d.join("Morii \u{1F4F7} \u{AE40}");
        std::fs::create_dir_all(&far).unwrap();
        let s = short_path(&far).unwrap();
        assert!(!s.to_string_lossy().starts_with(r"\\?\"), "{}", s.display());
        assert_eq!(file_id_of_path(&s).unwrap(), file_id_of_path(&far).unwrap());
        if s != far {
            assert!(ansi_exact(&s), "{}", s.display()); // this volume keeps short names
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn move_no_replace_refuses_existing_target_and_ensure_absent_detects_it() {
        let d = dir("mv");
        let a = d.join("a.xmp");
        let b = d.join("b.xmp");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        assert!(ensure_absent(&b).is_err());
        assert!(move_no_replace(&a, &b).is_err());
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        std::fs::remove_file(&b).unwrap();
        move_no_replace(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"a");
    }

    #[test]
    fn copy_never_overwrites_and_hashes() {
        let d = dir("copy");
        let src = d.join("s.jpg");
        std::fs::write(&src, vec![7u8; 3 << 20]).unwrap();
        let dst = d.join("backup.jpg");
        let mut f = open_lock(&src).unwrap();
        let h = copy_new_hashing(&mut f, &dst).unwrap();
        assert_eq!(h, hash_path(&src).unwrap());
        assert_eq!(hash_path(&dst).unwrap(), h);
        f.rewind().unwrap();
        assert_eq!(
            copy_new_hashing(&mut f, &dst).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    #[allow(clippy::permissions_set_readonly_false)] // Windows-only crate: clears FILE_ATTRIBUTE_READONLY
    fn probe_reports_links_and_read_only() {
        let d = dir("probe");
        let a = d.join("a.jpg");
        std::fs::write(&a, b"x").unwrap();
        std::fs::hard_link(&a, d.join("b.jpg")).unwrap();
        let mut perm = std::fs::metadata(&a).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&a, perm.clone()).unwrap();
        let p = probe(&a).unwrap();
        assert_eq!(p.links, 2);
        assert!(p.read_only);
        assert!(!p.reparse_point);
        perm.set_readonly(false);
        std::fs::set_permissions(&a, perm).unwrap();
    }

    #[test]
    fn wide_adds_verbatim_prefixes() {
        let w = |s: &str| {
            String::from_utf16_lossy(&wide(Path::new(s)))
                .trim_end_matches('\0')
                .to_owned()
        };
        assert_eq!(w(r"C:\a\b.jpg"), r"\\?\C:\a\b.jpg");
        assert_eq!(w("C:/a/b.jpg"), r"\\?\C:\a\b.jpg");
        assert_eq!(w(r"\\nas\share\x.jpg"), r"\\?\UNC\nas\share\x.jpg");
        assert_eq!(w(r"\\?\C:\x"), r"\\?\C:\x");
    }

    #[test]
    fn volume_root_and_kind_of_a_local_folder() {
        let d = dir("vol");
        let root = volume_root(&d.join("x.jpg")).unwrap();
        assert!(d.starts_with(&root), "{} / {}", d.display(), root.display());
        // a local fixed disk: SSD or HDD when the driver answers; never network or removable
        let k = volume_kind(&root);
        assert!(
            matches!(k, VolumeKind::Ssd | VolumeKind::Hdd | VolumeKind::Unknown),
            "{k:?}"
        );
        assert!(k.io_limit() >= 1);
        eprintln!("{} -> {k:?}", root.display());
    }

    #[test]
    fn volume_space_and_fill_guard() {
        let d = dir("space");
        let s = volume_space(&d).unwrap();
        assert!(s.total > 0 && s.free <= s.total);
        if s.total > FILL_MAX_VOLUME {
            // a normal disk is never filled, and nothing is created
            assert!(fill_volume(&d).is_err());
            assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        }
        assert!(is_disk_full(&io::Error::from_raw_os_error(112)));
        assert!(is_disk_full(&io::Error::from_raw_os_error(39)));
        assert!(!is_disk_full(&io::Error::from_raw_os_error(5)));
    }

    #[test]
    fn tokens_are_unique_hex() {
        let a = random_token().unwrap();
        assert_eq!(a.len(), 16);
        assert_ne!(a, random_token().unwrap());
        assert_eq!(
            sibling_name(Path::new(r"C:\p\IMG_1.JPG"), ".mmbak-", "ab"),
            PathBuf::from(r"C:\p\IMG_1.mmbak-ab.JPG")
        );
    }
}

#[cfg(test)]
mod reparse_tests {
    use super::*;

    #[test]
    fn reparse_tags_are_classified() {
        assert_eq!(Reparse::of_tag(0), Reparse::None);
        assert_eq!(Reparse::of_tag(0xA000_000C), Reparse::Link); // symbolic link
        assert_eq!(Reparse::of_tag(0xA000_0003), Reparse::Link); // junction
        assert_eq!(Reparse::of_tag(0x9000_001A), Reparse::Cloud);
        assert_eq!(Reparse::of_tag(0x9000_601A), Reparse::Cloud); // CLOUD_6 (OneDrive)
        assert_eq!(Reparse::of_tag(0x8000_0013), Reparse::Other); // deduplication
    }

    /// A junction is probed as a link and refused as one; a placeholder is refused as a
    /// placeholder before anything else, a read-only file last.
    #[test]
    fn probe_names_the_reason() {
        let d = std::env::temp_dir().join(format!("mm-reparse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("target")).unwrap();
        let j = d.join("j");
        let ok = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&j)
            .arg(d.join("target"))
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J failed");
        let pr = probe(&j).unwrap();
        assert_eq!(pr.reparse, Reparse::Link);
        assert_eq!(
            pr.refusal(),
            Some("symbolic link or junction (not written)")
        );
        let f = d.join("a.jpg");
        std::fs::write(&f, b"a").unwrap();
        let mut pr = probe(&f).unwrap();
        assert_eq!((pr.reparse, pr.refusal()), (Reparse::None, None));
        pr.read_only = true;
        assert!(pr.refusal().unwrap().starts_with("read-only"));
        pr.cloud_placeholder = true;
        assert!(pr.refusal().unwrap().starts_with("cloud placeholder"));
        std::fs::remove_dir(&j).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}
