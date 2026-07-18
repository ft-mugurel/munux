//! File descriptors and open-file objects (U1–U2).
//!
//! v0.1: one global FD table (becomes per-process in U5).
//! WRITE/READ go through this table.

use core::arch::asm;

use crate::console;
use crate::interrupts::keyboard::init as kbd;

/// Maximum open FDs per table.
pub const FD_MAX: usize = 32;

pub const STDIN_FILENO: usize = 0;
pub const STDOUT_FILENO: usize = 1;
pub const STDERR_FILENO: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    None,
    /// VGA text console (write) + keyboard (read in U2).
    Console,
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
            FileKind::None => Err(FdError::BadFd),
        }
    }

    /// Read up to `buf.len()` bytes. Console: block until ≥1 byte, then drain available.
    pub fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, FdError> {
        let file = self.get_mut(fd).ok_or(FdError::BadFd)?;
        if !file.readable {
            return Err(FdError::BadFd);
        }
        match file.kind {
            FileKind::Console => Ok(console_read(buf)),
            FileKind::None => Err(FdError::BadFd),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdError {
    BadFd,
    Fault,
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

/// Blocking console read: wait for keyboard data, then copy what is available.
///
/// Policy (docs/ABI.md v0.1 / U2):
/// - If the ring buffer is empty, `sti; hlt` until an IRQ delivers bytes.
/// - Then copy `min(available, buf.len())` without waiting for a full line.
/// Line buffering is left to userspace (future `/bin/sh`).
fn console_read(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    // Wait for at least one byte (interrupts must be on — syscall clears IF via FMASK).
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

pub fn sys_write_slice(fd: u64, data: &[u8]) -> Result<usize, FdError> {
    if !is_ready() || fd > (FD_MAX as u64) {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.write(fd as usize, data))
}

/// Read into a kernel-side temporary buffer; caller copies out to user.
pub fn sys_read_into(fd: u64, buf: &mut [u8]) -> Result<usize, FdError> {
    if !is_ready() || fd > (FD_MAX as u64) {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.read(fd as usize, buf))
}

pub fn sys_close(fd: u64) -> Result<(), FdError> {
    if !is_ready() || fd > (FD_MAX as u64) {
        return Err(FdError::BadFd);
    }
    if with_current(|t| t.close(fd as usize)) {
        Ok(())
    } else {
        Err(FdError::BadFd)
    }
}
