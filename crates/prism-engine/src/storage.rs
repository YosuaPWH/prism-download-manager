//! Sparse part-file creation and positioned I/O.
//!
//! * Windows: the file is marked sparse (`FSCTL_SET_SPARSE`) before `set_len`, so writes at far
//!   offsets do not trigger NTFS zero-filling from the valid-data length. Keep that flag when
//!   closing, including on pause/error: clearing it materializes every unwritten hole (#70).
//! * Unix: `set_len` creates holes rather than reserving the whole download on disk.
//!   Free space is checked separately; actual writes can still report an out-of-space error.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use crate::error::EngineError;

/// Creates (or truncates) the part file and sizes it. `create_new` fails if it exists.
pub(crate) fn create_part_file(
    path: &Path,
    size: Option<u64>,
    create_new: bool,
) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true);
    if create_new {
        opts.create_new(true);
    } else {
        opts.create(true).truncate(true);
    }
    let file = opts.open(path)?;
    prepare_sparse(&file);
    if let Some(size) = size.filter(|s| *s > 0) {
        allocate(&file, size)?;
    }
    Ok(file)
}

/// Opens an existing part file for resuming.
pub(crate) fn open_part_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    // Repair partial files paused by older versions, which cleared their sparse flag.
    prepare_sparse(&file);
    Ok(file)
}

fn allocate(file: &File, size: u64) -> io::Result<()> {
    file.set_len(size)
}

fn prepare_sparse(file: &File) {
    #[cfg(windows)]
    if let Err(e) = set_sparse(file) {
        // Some volumes (FAT/exFAT) do not support sparse files. Make the fallback visible
        // in diagnostics rather than silently claiming that writes will stay sparse.
        tracing::warn!("could not mark the download file sparse: {e}");
    }
    #[cfg(not(windows))]
    let _ = file;
}

/// Marks a file as sparse on NTFS/ReFS. Never clear it on a partial file.
#[cfg(windows)]
fn set_sparse(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FILE_SET_SPARSE_BUFFER, FSCTL_SET_SPARSE};

    let input = FILE_SET_SPARSE_BUFFER { SetSparse: true };
    let mut returned = 0u32;
    // SAFETY: the handle is valid for the lifetime of `file`; the input buffer is a properly
    // initialized FILE_SET_SPARSE_BUFFER of the declared size; no output buffer is used and the
    // call is synchronous (no OVERLAPPED).
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_SPARSE,
            &input as *const FILE_SET_SPARSE_BUFFER as *const core::ffi::c_void,
            std::mem::size_of::<FILE_SET_SPARSE_BUFFER>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Writes all of `buf` at `offset` without touching a shared cursor.
#[cfg(windows)]
pub(crate) fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole buffer",
                ));
            }
            Ok(n) => {
                buf = &buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Writes all of `buf` at `offset` without touching a shared cursor.
#[cfg(unix)]
pub(crate) fn write_all_at(file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

/// Reads exactly `buf.len()` bytes at `offset`.
#[cfg(windows)]
pub(crate) fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_read(buf, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(n) => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Reads exactly `buf.len()` bytes at `offset`.
#[cfg(unix)]
pub(crate) fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

/// Fails with [`EngineError::NoSpace`] when the volume holding `dir` has less than `needed`
/// bytes available. Unknown free space (e.g. network shares that do not report it) passes.
pub(crate) fn check_free_space(dir: &Path, needed: u64) -> Result<(), EngineError> {
    if needed == 0 {
        return Ok(());
    }
    match fs4::available_space(dir) {
        Ok(available) if available < needed => Err(EngineError::NoSpace { needed, available }),
        _ => Ok(()),
    }
}

/// Moves `from` to `to`, replacing `to` if it exists (atomic on the same volume).
pub(crate) fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Moves `from` to `to` only if `to` does not exist. Returns `Ok(false)` when it exists.
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
        let wide = |p: &Path| {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let (f, t) = (wide(from), wide(to));
        // SAFETY: both buffers are NUL-terminated UTF-16 strings that outlive the call.
        let ok = unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), MOVEFILE_WRITE_THROUGH) };
        if ok != 0 {
            return Ok(true);
        }
        let e = io::Error::last_os_error();
        // ERROR_FILE_EXISTS (80) / ERROR_ALREADY_EXISTS (183)
        if matches!(e.raw_os_error(), Some(80) | Some(183)) {
            Ok(false)
        } else {
            Err(e)
        }
    }
    #[cfg(not(windows))]
    {
        // hard_link + remove gives "no replace" semantics on POSIX file systems.
        match std::fs::hard_link(from, to) {
            Ok(()) => {
                std::fs::remove_file(from)?;
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            // File systems without hard links: check-then-rename (small race window).
            Err(_) if !to.exists() => std::fs::rename(from, to).map(|_| true),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_write_read_rename() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.prismpart");
        let f = create_part_file(&p, Some(10 << 20), true).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 10 << 20);
        assert!(
            create_part_file(&p, Some(1), true).is_err(),
            "create_new must not clobber"
        );
        write_all_at(&f, b"tail", (10 << 20) - 4).unwrap();
        write_all_at(&f, b"head", 0).unwrap();
        let mut buf = [0u8; 4];
        read_exact_at(&f, &mut buf, (10 << 20) - 4).unwrap();
        assert_eq!(&buf, b"tail");
        read_exact_at(&f, &mut buf, 0).unwrap();
        assert_eq!(&buf, b"head");
        f.sync_all().unwrap();
        drop(f);
        let fin = dir.path().join("x");
        std::fs::write(&fin, b"old").unwrap();
        assert!(!rename_no_replace(&p, &fin).unwrap());
        rename_replace(&p, &fin).unwrap();
        assert_eq!(std::fs::metadata(&fin).unwrap().len(), 10 << 20);
        let other = dir.path().join("y");
        std::fs::write(dir.path().join("z"), b"1").unwrap();
        assert!(rename_no_replace(&dir.path().join("z"), &other).unwrap());
        assert!(other.exists());
    }

    #[cfg(unix)]
    #[test]
    fn sizing_and_resuming_a_large_file_preserves_holes() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.prismpart");
        let size = 1 << 30;
        let file = create_part_file(&path, Some(size), true).unwrap();
        write_all_at(&file, b"head", 0).unwrap();
        write_all_at(&file, b"tail", size - 4).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let file = open_part_file(&path).unwrap();
        assert_eq!(file.metadata().unwrap().len(), size);
        assert!(file.metadata().unwrap().blocks() * 512 < 1 << 20);
        let mut hole = [1u8; 4];
        read_exact_at(&file, &mut hole, size / 2).unwrap();
        assert_eq!(hole, [0; 4]);
    }

    #[test]
    fn free_space_check() {
        let dir = tempfile::tempdir().unwrap();
        check_free_space(dir.path(), 1).unwrap();
        assert!(matches!(
            check_free_space(dir.path(), u64::MAX),
            Err(EngineError::NoSpace { .. })
        ));
    }
}
