//! S2 spike: Windows file-transaction primitives and a crash-injectable single-file transaction.
//!
//! Throwaway research code. The product equivalent will live in crates/mm-fs + mm-store.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ID_INFO, FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo,
    FileRenameInfoEx, FlushFileBuffers, GetFileInformationByHandle, GetFileInformationByHandleEx,
    BY_HANDLE_FILE_INFORMATION, MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW, SetFileInformationByHandle,
};

pub mod txn;

pub const FILE_RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 0x1;
pub const FILE_RENAME_FLAG_POSIX_SEMANTICS: u32 = 0x2;
pub const FILE_READ_ATTRIBUTES: u32 = 0x80;
pub const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

/// NUL-terminated wide path; absolute local paths get the `\\?\` prefix, UNC paths `\\?\UNC\`.
pub fn wide(p: &Path) -> Vec<u16> {
    let s = p.as_os_str().to_string_lossy().replace('/', "\\");
    let s = if s.starts_with(r"\\?\") {
        s
    } else if let Some(rest) = s.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")
    } else {
        format!(r"\\?\{s}")
    };
    std::ffi::OsStr::new(&s).encode_wide().chain(std::iter::once(0)).collect()
}

pub fn last_error() -> u32 {
    unsafe { GetLastError() }
}

/// Lock handle: read access only, deny write sharing, allow read + delete/rename sharing.
/// ExifTool's own read opens (GENERIC_READ, share READ|WRITE) are compatible with it.
pub fn open_lock(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(GENERIC_READ)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(path)
}

pub fn open_with(path: &Path, access: u32, share: u32) -> io::Result<File> {
    OpenOptions::new().access_mode(access).share_mode(share).open(path)
}

/// Attribute-only open that never conflicts with other openers (for identity checks).
pub fn open_attr(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileId {
    pub volume: u64,
    pub id: [u8; 16],
}

impl FileId {
    pub fn hex(&self) -> String {
        format!("{:016x}:{}", self.volume, self.id.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }
}

pub fn file_id(f: &File) -> io::Result<FileId> {
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
        Ok(FileId { volume: info.VolumeSerialNumber, id: info.FileId.Identifier })
    }
}

pub fn file_id_of_path(p: &Path) -> io::Result<FileId> {
    file_id(&open_attr(p)?)
}

pub fn link_count(f: &File) -> io::Result<u32> {
    unsafe {
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        if GetFileInformationByHandle(f.as_raw_handle() as HANDLE, &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info.nNumberOfLinks)
    }
}

/// `ReplaceFileW(replaced, replacement, backup, 0)`; Err(win32 error code).
pub fn replace_file(replaced: &Path, replacement: &Path, backup: &Path) -> Result<(), u32> {
    let (a, b, c) = (wide(replaced), wide(replacement), wide(backup));
    let ok = unsafe { ReplaceFileW(a.as_ptr(), b.as_ptr(), c.as_ptr(), 0, std::ptr::null(), std::ptr::null()) };
    if ok == 0 { Err(last_error()) } else { Ok(()) }
}

/// Single-step replace: rename `replacement` over `target` with POSIX semantics (FileRenameInfoEx).
pub fn posix_replace(replacement: &Path, target: &Path) -> Result<(), u32> {
    let f = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(replacement)
        .map_err(|e| e.raw_os_error().unwrap_or(-1) as u32)?;
    let name: Vec<u16> = wide(target);
    let name = &name[..name.len() - 1]; // no NUL; length in bytes
    let header = std::mem::size_of::<FILE_RENAME_INFO>();
    let size = header + name.len() * 2;
    let mut buf = vec![0u8; size];
    unsafe {
        let info = buf.as_mut_ptr() as *mut FILE_RENAME_INFO;
        (*info).Anonymous.Flags = FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = (name.len() * 2) as u32;
        let dst = std::ptr::addr_of_mut!((*info).FileName) as *mut u16;
        std::ptr::copy_nonoverlapping(name.as_ptr(), dst, name.len());
        if SetFileInformationByHandle(f.as_raw_handle() as HANDLE, FileRenameInfoEx, buf.as_ptr() as *const _, size as u32) == 0 {
            return Err(last_error());
        }
    }
    Ok(())
}

/// Rename without replacing an existing target.
pub fn move_no_replace(src: &Path, dst: &Path) -> Result<(), u32> {
    let (a, b) = (wide(src), wide(dst));
    let ok = unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 { Err(last_error()) } else { Ok(()) }
}

pub fn flush_path(p: &Path) -> io::Result<()> {
    let f = OpenOptions::new().access_mode(GENERIC_WRITE).share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE).open(p)?;
    flush(&f)
}

pub fn flush(f: &File) -> io::Result<()> {
    if unsafe { FlushFileBuffers(f.as_raw_handle() as HANDLE) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn hash_reader(mut r: impl Read) -> io::Result<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(*h.finalize().as_bytes())
}

pub fn hash_path(p: &Path) -> io::Result<[u8; 32]> {
    hash_reader(open_with(p, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)?)
}

pub fn hex(h: &[u8; 32]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// Copy `src` (already open) to a new file `dst` (create_new), hashing on the fly; flushes `dst`.
pub fn copy_new_hashing(src: &mut File, dst: &Path) -> io::Result<[u8; 32]> {
    let mut out = OpenOptions::new().write(true).create_new(true).open(dst)?;
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
}

pub fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

pub fn sibling(p: &Path, suffix: &str) -> PathBuf {
    let stem = p.file_stem().unwrap().to_string_lossy();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    p.with_file_name(format!("{stem}{suffix}{ext}"))
}
