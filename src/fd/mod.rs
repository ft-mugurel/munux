//! File descriptors and open-file objects (U1–U3).
//!
//! v0.2: global FD table; console + read-only ext2 files.

use core::arch::asm;

use crate::console;
use crate::fs;
use crate::fs::ext2;
use crate::interrupts::keyboard::init as kbd;

pub const FD_MAX: usize = 32;

pub const STDIN_FILENO: usize = 0;
pub const STDOUT_FILENO: usize = 1;
pub const STDERR_FILENO: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    None,
    /// VGA console (write) + keyboard (read).
    Console,
    /// Read-only ext2 regular file.
    Ext2File { ino: u32 },
}

#[derive(Clone, Copy)]
pub struct File {
    pub kind: FileKind,
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

    pub fn ext2_ro(ino: u32) -> Self {
        Self {
            kind: FileKind::Ext2File { ino },
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

    /// Install a new open file; returns FD number.
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
        let mut n = 0;
        for f in &self.entries {
            if f.kind != FileKind::None {
                n += 1;
            }
        }
        n
    }

    pub fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, FdError> {
        let file = self.get_mut(fd).ok_or(FdError::BadFd)?;
        if !file.writable {
            return Err(FdError::BadFd);
        }
        match file.kind {
            FileKind::Console => Ok(console_write(data)),
            FileKind::Ext2File { .. } => Err(FdError::BadFd), // read-only for now
            FileKind::None => Err(FdError::BadFd),
        }
    }

    pub fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, FdError> {
        // Split borrow: take kind/offset, then mutate offset after read.
        let (kind, offset, readable) = {
            let file = self.get(fd).ok_or(FdError::BadFd)?;
            if !file.readable {
                return Err(FdError::BadFd);
            }
            (file.kind, file.offset, file.readable)
        };
        let _ = readable;

        let n = match kind {
            FileKind::Console => console_read(buf),
            FileKind::Ext2File { ino } => ext2_read(ino, offset, buf)?,
            FileKind::None => return Err(FdError::BadFd),
        };

        if let Some(file) = self.get_mut(fd) {
            file.offset = file.offset.saturating_add(n as u64);
        }
        Ok(n)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdError {
    BadFd,
    Fault,
    NoEnt,
    IsDir,
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
        if b == b'\n' {
            console::put_char(b'\n');
            n += 1;
        } else if (32..127).contains(&b) || b == b'\t' {
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
    while kbd::buffered_len() == 0 {
        unsafe {
            asm!("sti; hlt", options(nomem, nostack));
        }
    }
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

fn ext2_read(ino: u32, offset: u64, buf: &mut [u8]) -> Result<usize, FdError> {
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

/// Open path relative to cwd (absolute if starts with '/'). Read-only.
/// Linux O_RDONLY = 0; we reject write-only/rdwr for now.
pub fn open_path(path: &str, flags: u64) -> Result<usize, FdError> {
    if !is_ready() {
        return Err(FdError::BadFd);
    }
    if !fs::is_ready() {
        return Err(FdError::NoEnt);
    }
    // O_ACCMODE = 3; only allow O_RDONLY (0)
    let acc = flags & 3;
    if acc != 0 {
        // no write support yet
        return Err(FdError::Inval);
    }
    if path.is_empty() {
        return Err(FdError::NoEnt);
    }

    let cwd = fs::path::cwd_inode();
    let ino = fs::ext2::resolve_path(cwd, path).map_err(|_| FdError::NoEnt)?;
    if fs::ext2::inode_is_dir(ino) {
        // directories need getdents (U4)
        return Err(FdError::IsDir);
    }

    with_current(|t| t.install(File::ext2_ro(ino)))
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
