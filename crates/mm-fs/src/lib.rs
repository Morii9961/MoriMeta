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
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ID_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo,
    FlushFileBuffers, GetDiskFreeSpaceExW, GetDriveTypeW, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetVolumePathNameW, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    ReplaceFileW,
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

/// Properties relevant to the pre-check (SAFETY_MODEL §4.1 step 2, §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub read_only: bool,
    pub reparse_point: bool,
    pub cloud_placeholder: bool,
    pub links: u32,
    pub size: u64,
}

pub fn probe(p: &Path) -> io::Result<Probe> {
    let f = open_attr(p)?;
    let m = f.metadata()?;
    let a = m.file_attributes();
    Ok(Probe {
        read_only: a & FILE_ATTRIBUTE_READONLY != 0,
        reparse_point: a & FILE_ATTRIBUTE_REPARSE_POINT != 0,
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
