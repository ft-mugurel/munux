//! File descriptors (U1–U4): console, files, directories + getdents64.

use crate::console;
use crate::fs;
use crate::fs::ext2;
use crate::interrupts::keyboard::init as kbd;

pub const FD_MAX: usize = 32;

pub const STDIN_FILENO: usize = 0;
pub const STDOUT_FILENO: usize = 1;
pub const STDERR_FILENO: usize = 2;

/// Linux open flags (subset).
pub const O_RDONLY: u64 = 0;
pub const O_WRONLY: u64 = 1;
pub const O_RDWR: u64 = 2;
pub const O_CREAT: u64 = 0o100;
pub const O_TRUNC: u64 = 0o1000;
/// Linux open flag O_DIRECTORY.
pub const O_DIRECTORY: u64 = 0o200000;
/// Linux O_ACCMODE
const O_ACCMODE: u64 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    None,
    Console,
    Ext2File { ino: u32 },
    Ext2Dir { ino: u32 },
}

#[derive(Clone, Copy)]
pub struct File {
    pub kind: FileKind,
    /// File byte offset, or directory cookie for getdents64.
    pub offset: u64,
    pub readable: bool,
    pub writable: bool,
}

impl File {
    pub const fn closed() -> Self {
        Self {
            kind: FileKind::None,
            offset: 0,
            readable: false,
            writable: false,
        }
    }

    pub const fn console_stdin() -> Self {
        Self {
            kind: FileKind::Console,
            offset: 0,
            readable: true,
            writable: false,
        }
    }

    pub const fn console_stdout() -> Self {
        Self {
            kind: FileKind::Console,
            offset: 0,
            readable: false,
            writable: true,
        }
    }

    pub fn ext2_file(ino: u32, readable: bool, writable: bool) -> Self {
        Self {
            kind: FileKind::Ext2File { ino },
            offset: 0,
            readable,
            writable,
        }
    }

    pub fn ext2_dir(ino: u32) -> Self {
        Self {
            kind: FileKind::Ext2Dir { ino },
            offset: 0,
            readable: true,
            writable: false,
        }
    }
}

pub struct FdTable {
    entries: [File; FD_MAX],
}

impl FdTable {
    pub const fn new() -> Self {
        Self {
            entries: [File::closed(); FD_MAX],
        }
    }

    pub fn install_stdio(&mut self) {
        self.entries[STDIN_FILENO] = File::console_stdin();
        self.entries[STDOUT_FILENO] = File::console_stdout();
        self.entries[STDERR_FILENO] = File::console_stdout();
        for i in 3..FD_MAX {
            self.entries[i] = File::closed();
        }
    }

    pub fn get(&self, fd: usize) -> Option<&File> {
        if fd >= FD_MAX {
            return None;
        }
        let f = &self.entries[fd];
        if f.kind == FileKind::None {
            None
        } else {
            Some(f)
        }
    }

    pub fn get_mut(&mut self, fd: usize) -> Option<&mut File> {
        if fd >= FD_MAX {
            return None;
        }
        let f = &mut self.entries[fd];
        if f.kind == FileKind::None {
            None
        } else {
            Some(f)
        }
    }

    fn alloc_slot(&mut self) -> Option<usize> {
        for i in 0..FD_MAX {
            if self.entries[i].kind == FileKind::None {
                return Some(i);
            }
        }
        None
    }

    pub fn install(&mut self, file: File) -> Result<usize, FdError> {
        let i = self.alloc_slot().ok_or(FdError::NoMem)?;
        self.entries[i] = file;
        Ok(i)
    }

    pub fn close(&mut self, fd: usize) -> bool {
        if fd >= FD_MAX || self.entries[fd].kind == FileKind::None {
            return false;
        }
        self.entries[fd] = File::closed();
        true
    }

    pub fn open_count(&self) -> usize {
        self.entries.iter().filter(|f| f.kind != FileKind::None).count()
    }

    pub fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, FdError> {
        let (kind, offset) = {
            let file = self.get(fd).ok_or(FdError::BadFd)?;
            if !file.writable {
                return Err(FdError::BadFd);
            }
            (file.kind, file.offset)
        };
        let n = match kind {
            FileKind::Console => console_write(data),
            FileKind::Ext2File { ino } => {
                if offset > u32::MAX as u64 {
                    return Ok(0);
                }
                match crate::fs::ext2_write::write_file_at(ino, offset as u32, data) {
                    Ok(n) => n,
                    Err("file too large") => return Err(FdError::Inval),
                    Err("is a directory") => return Err(FdError::IsDir),
                    Err(_) => return Err(FdError::Fault),
                }
            }
            FileKind::Ext2Dir { .. } => return Err(FdError::IsDir),
            FileKind::None => return Err(FdError::BadFd),
        };
        if let Some(file) = self.get_mut(fd) {
            file.offset = file.offset.saturating_add(n as u64);
        }
        Ok(n)
    }

    pub fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, FdError> {
        let (kind, offset) = {
            let file = self.get(fd).ok_or(FdError::BadFd)?;
            if !file.readable {
                return Err(FdError::BadFd);
            }
            (file.kind, file.offset)
        };

        let n = match kind {
            FileKind::Console => console_read(buf),
            FileKind::Ext2File { ino } => ext2_file_read(ino, offset, buf)?,
            FileKind::Ext2Dir { .. } => return Err(FdError::IsDir),
            FileKind::None => return Err(FdError::BadFd),
        };

        if let Some(file) = self.get_mut(fd) {
            file.offset = file.offset.saturating_add(n as u64);
        }
        Ok(n)
    }

    /// Linux getdents64(fd, dirp, count) — fill buffer, advance dir offset cookie.
    pub fn getdents64(&mut self, fd: usize, out: &mut [u8]) -> Result<usize, FdError> {
        let (ino, mut cookie) = {
            let file = self.get(fd).ok_or(FdError::BadFd)?;
            match file.kind {
                FileKind::Ext2Dir { ino } => (ino, file.offset as u32),
                FileKind::Ext2File { .. } => return Err(FdError::NotDir),
                FileKind::Console => return Err(FdError::NotDir),
                FileKind::None => return Err(FdError::BadFd),
            }
        };

        if out.len() < 24 {
            return Err(FdError::Inval);
        }

        let mut written = 0usize;
        loop {
            let ent = match ext2::dir_next_entry(ino, cookie) {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(_) => return Err(FdError::Fault),
            };

            // linux_dirent64: ino u64, off i64, reclen u16, type u8, name...
            let name_len = ent.name_len as usize;
            let reclen = (19 + name_len + 1 + 7) & !7; // align 8, include NUL
            if written + reclen > out.len() {
                if written == 0 {
                    return Err(FdError::Inval); // buffer too small for one entry
                }
                break;
            }

            let base = written;
            // d_ino
            out[base..base + 8].copy_from_slice(&(ent.ino as u64).to_le_bytes());
            // d_off = next cookie
            out[base + 8..base + 16].copy_from_slice(&(ent.next_off as i64).to_le_bytes());
            // d_reclen
            out[base + 16..base + 18].copy_from_slice(&(reclen as u16).to_le_bytes());
            // d_type
            out[base + 18] = ent.d_type;
            // d_name + NUL
            out[base + 19..base + 19 + name_len].copy_from_slice(&ent.name[..name_len]);
            out[base + 19 + name_len] = 0;
            // pad
            for b in out.iter_mut().take(base + reclen).skip(base + 20 + name_len) {
                *b = 0;
            }

            written += reclen;
            cookie = ent.next_off;
        }

        if let Some(file) = self.get_mut(fd) {
            file.offset = cookie as u64;
        }
        Ok(written)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdError {
    BadFd,
    Fault,
    NoEnt,
    IsDir,
    NotDir,
    NoMem,
    Inval,
}

static mut CURRENT: FdTable = FdTable::new();
static mut READY: bool = false;

pub fn init() {
    unsafe {
        let t = &mut *core::ptr::addr_of_mut!(CURRENT);
        t.install_stdio();
        READY = true;
    }
}

pub fn is_ready() -> bool {
    unsafe { READY }
}

pub fn with_current<F, R>(f: F) -> R
where
    F: FnOnce(&mut FdTable) -> R,
{
    unsafe { f(&mut *core::ptr::addr_of_mut!(CURRENT)) }
}

pub fn open_count() -> usize {
    with_current(|t| t.open_count())
}

fn console_write(data: &[u8]) -> usize {
    let mut n = 0usize;
    for &b in data {
        // Shells need BS/DEL for line edit and FF for clear.
        if b == b'\n' || b == b'\t' {
            console::put_char(b);
            n += 1;
        } else if b == 0x08 || b == 0x7F {
            console::put_char(0x08);
            n += 1;
        } else if b == 0x0C {
            console::clear();
            n += 1;
        } else if (32..127).contains(&b) {
            console::put_char(b);
            n += 1;
        }
    }
    n
}

fn console_read(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    // IRQ-safe wait: re-checks buffer after every hlt wake (see keyboard::wait_for_input).
    kbd::wait_for_input();
    let mut n = 0usize;
    while n < buf.len() {
        match kbd::pop_char() {
            Some(b) => {
                buf[n] = b;
                n += 1;
            }
            None => break,
        }
    }
    n
}

fn ext2_file_read(ino: u32, offset: u64, buf: &mut [u8]) -> Result<usize, FdError> {
    if !fs::is_ready() {
        return Err(FdError::NoEnt);
    }
    if offset > u32::MAX as u64 {
        return Ok(0);
    }
    match ext2::read_file(ino, offset as u32, buf) {
        Ok(n) => Ok(n),
        Err(_) => Err(FdError::Fault),
    }
}

/// Open path. Files → Ext2File; directories → Ext2Dir (for getdents64).
/// Supports O_RDONLY / O_WRONLY / O_RDWR, O_CREAT, O_TRUNC, O_DIRECTORY.
pub fn open_path(path: &str, flags: u64) -> Result<usize, FdError> {
    if !is_ready() {
        return Err(FdError::BadFd);
    }
    if !fs::is_ready() {
        return Err(FdError::NoEnt);
    }
    let acc = flags & O_ACCMODE;
    if acc > O_RDWR {
        return Err(FdError::Inval);
    }
    if path.is_empty() {
        return Err(FdError::NoEnt);
    }

    let readable = acc == O_RDONLY || acc == O_RDWR;
    let writable = acc == O_WRONLY || acc == O_RDWR;

    let cwd = fs::path::cwd_inode();
    let ino = match fs::ext2::resolve_path(cwd, path) {
        Ok(i) => i,
        Err(_) => {
            if flags & O_CREAT == 0 {
                return Err(FdError::NoEnt);
            }
            // Create empty file then re-resolve.
            fs::ext2_write::touch(cwd, path).map_err(|_| FdError::NoEnt)?
        }
    };
    let is_dir = fs::ext2::inode_is_dir(ino);

    if flags & O_DIRECTORY != 0 && !is_dir {
        return Err(FdError::NotDir);
    }
    if is_dir && writable {
        return Err(FdError::IsDir);
    }

    if !is_dir && (flags & O_TRUNC) != 0 && writable {
        let _ = fs::ext2_write::truncate_file(ino);
    }

    let file = if is_dir {
        File::ext2_dir(ino)
    } else {
        File::ext2_file(ino, readable, writable)
    };
    with_current(|t| t.install(file))
}

pub fn sys_write_slice(fd: u64, data: &[u8]) -> Result<usize, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.write(fd as usize, data))
}

pub fn sys_read_into(fd: u64, buf: &mut [u8]) -> Result<usize, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.read(fd as usize, buf))
}

pub fn sys_close(fd: u64) -> Result<(), FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    if with_current(|t| t.close(fd as usize)) {
        Ok(())
    } else {
        Err(FdError::BadFd)
    }
}

pub fn sys_open_path(path: &str, flags: u64) -> Result<usize, FdError> {
    open_path(path, flags)
}

pub fn sys_getdents64(fd: u64, buf: &mut [u8]) -> Result<usize, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.getdents64(fd as usize, buf))
}
