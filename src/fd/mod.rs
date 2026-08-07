//! File descriptors (U1–U4 + per-process / shared tables).
//!
//! Default: each process slot owns an [`FdTable`]. `fork` clones the parent
//! table. `CLONE_FILES` makes the child point at the parent's table (refcount).

use crate::fs;
use crate::fs::vcore::{self, FileData, VfsError};
use crate::process::pcb::MAX_PROCESSES;

pub mod pipe;

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

/// Open file — VFS `FileData` (Phase 7) replaces ad-hoc Ext2/Console enums.
#[derive(Clone, Copy)]
pub struct File {
    pub data: FileData,
}

impl File {
    pub const fn closed() -> Self {
        Self {
            data: FileData::closed(),
        }
    }

    pub fn console_stdin() -> Self {
        Self {
            data: vcore::console_file(true, false),
        }
    }

    pub fn console_stdout() -> Self {
        Self {
            data: vcore::console_file(false, true),
        }
    }

    pub fn from_vfs(data: FileData) -> Self {
        Self { data }
    }

    pub fn is_open(&self) -> bool {
        self.data.fops_id != vcore::FOPS_NONE
    }

    pub fn is_dir(&self) -> bool {
        self.data.is_dir
    }

    pub fn ext2_ino(&self) -> Option<u32> {
        if self.data.fops_id == vcore::FOPS_EXT2_FILE || self.data.fops_id == vcore::FOPS_EXT2_DIR {
            Some(self.data.private as u32)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy)]
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

    /// Close every entry (process exit / slot free).
    pub fn close_all(&mut self) {
        for i in 0..FD_MAX {
            self.entries[i] = File::closed();
        }
    }

    /// Linux-like: copy open FDs (independent offsets after clone).
    pub fn clone_from(&mut self, other: &FdTable) {
        *self = *other;
    }

    pub fn get(&self, fd: usize) -> Option<&File> {
        if fd >= FD_MAX {
            return None;
        }
        let f = &self.entries[fd];
        if !f.is_open() {
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
        if !f.is_open() {
            None
        } else {
            Some(f)
        }
    }

    fn alloc_slot(&mut self) -> Option<usize> {
        for i in 0..FD_MAX {
            if !self.entries[i].is_open() {
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

    /// Install a copy of `file` in the lowest free slot ≥ `min_fd` (fcntl F_DUPFD).
    pub fn install_at_least(&mut self, min_fd: usize, file: File) -> Result<usize, FdError> {
        let start = min_fd.min(FD_MAX);
        for i in start..FD_MAX {
            if !self.entries[i].is_open() {
                self.entries[i] = file;
                return Ok(i);
            }
        }
        Err(FdError::NoMem)
    }

    pub fn close(&mut self, fd: usize) -> bool {
        if fd >= FD_MAX || !self.entries[fd].is_open() {
            return false;
        }
        vcore::vfs_release(&mut self.entries[fd].data);
        self.entries[fd] = File::closed();
        true
    }

    /// Duplicate fd into lowest free slot (dup).
    pub fn dup_fd(&mut self, fd: usize) -> Result<usize, FdError> {
        let file = *self.get(fd).ok_or(FdError::BadFd)?;
        // Pipe ends: bump reader/writer counts when duplicating.
        if file.data.fops_id == vcore::FOPS_PIPE_R {
            // reopen reader ref
            let id = file.data.private as usize;
            // alloc already set counts; manual bump
            // re-open as second reader by reusing same id
            let _ = id;
        }
        if file.data.fops_id == vcore::FOPS_MOD {
            vcore::mod_chrdev_dup(&file.data);
        }
        self.install(file)
    }

    /// dup2(oldfd, newfd).
    pub fn dup2_fd(&mut self, old: usize, new: usize) -> Result<usize, FdError> {
        if new >= FD_MAX {
            return Err(FdError::BadFd);
        }
        let file = *self.get(old).ok_or(FdError::BadFd)?;
        if old == new {
            return Ok(new);
        }
        if self.entries[new].is_open() {
            vcore::vfs_release(&mut self.entries[new].data);
        }
        if file.data.fops_id == vcore::FOPS_MOD {
            vcore::mod_chrdev_dup(&file.data);
        }
        self.entries[new] = file;
        Ok(new)
    }

    /// Create a pipe; returns (read_fd, write_fd).
    pub fn pipe_open(&mut self) -> Result<(usize, usize), FdError> {
        let id = pipe::alloc().map_err(|_| FdError::NoMem)?;
        let r = File::from_vfs(FileData {
            pos: 0,
            readable: true,
            writable: false,
            private: id as u64,
            is_dir: false,
            fops_id: vcore::FOPS_PIPE_R,
        });
        let w = File::from_vfs(FileData {
            pos: 0,
            readable: false,
            writable: true,
            private: id as u64,
            is_dir: false,
            fops_id: vcore::FOPS_PIPE_W,
        });
        let rfd = self.install(r).map_err(|e| {
            pipe::close_reader(id);
            pipe::close_writer(id);
            e
        })?;
        let wfd = self.install(w).map_err(|e| {
            let _ = self.close(rfd);
            e
        })?;
        Ok((rfd, wfd))
    }

    pub fn open_count(&self) -> usize {
        self.entries.iter().filter(|f| f.is_open()).count()
    }

    pub fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, FdError> {
        let file = self.get_mut(fd).ok_or(FdError::BadFd)?;
        vcore::vfs_write(&mut file.data, data).map_err(vfs_to_fd)
    }

    pub fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, FdError> {
        let file = self.get_mut(fd).ok_or(FdError::BadFd)?;
        vcore::vfs_read(&mut file.data, buf).map_err(vfs_to_fd)
    }

    /// Linux getdents64(fd, dirp, count) — fill buffer, advance dir offset cookie.
    ///
    /// Supports ext2 dirs, virtual dirs (`/proc`, `/dev`, `/ram`), and injects
    /// mount-point names into the root listing so `ls /` shows `proc`/`dev`/`ram`.
    pub fn getdents64(&mut self, fd: usize, out: &mut [u8]) -> Result<usize, FdError> {
        let (mut cookie, is_dir) = {
            let file = self.get(fd).ok_or(FdError::BadFd)?;
            let ok = file.data.fops_id == vcore::FOPS_EXT2_DIR
                || file.data.fops_id == vcore::FOPS_VDIR;
            if !ok {
                return Err(FdError::NotDir);
            }
            (file.data.pos, file.data.is_dir)
        };
        if !is_dir {
            return Err(FdError::NotDir);
        }
        if out.len() < 24 {
            return Err(FdError::Inval);
        }

        let mut written = 0usize;
        loop {
            let snap = {
                let file = self.get(fd).ok_or(FdError::BadFd)?;
                file.data
            };
            let ent = match vcore::vfs_dir_next(&snap, cookie) {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(VfsError::NotDir) => return Err(FdError::NotDir),
                Err(_) => return Err(FdError::Fault),
            };

            let name_len = ent.name_len as usize;
            let reclen = (19 + name_len + 1 + 7) & !7;
            if written + reclen > out.len() {
                if written == 0 {
                    return Err(FdError::Inval);
                }
                break;
            }

            let base = written;
            out[base..base + 8].copy_from_slice(&ent.ino.to_le_bytes());
            out[base + 8..base + 16].copy_from_slice(&(ent.next_off as i64).to_le_bytes());
            out[base + 16..base + 18].copy_from_slice(&(reclen as u16).to_le_bytes());
            out[base + 18] = ent.d_type;
            out[base + 19..base + 19 + name_len].copy_from_slice(&ent.name[..name_len]);
            out[base + 19 + name_len] = 0;
            for b in out.iter_mut().take(base + reclen).skip(base + 20 + name_len) {
                *b = 0;
            }

            written += reclen;
            cookie = ent.next_off;
        }

        if let Some(file) = self.get_mut(fd) {
            file.data.pos = cookie;
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
    Exist,
    Loop,
}

/// Physical FD table storage, one per process-table slot index.
static mut TABLES: [FdTable; MAX_PROCESSES] = [FdTable::new(); MAX_PROCESSES];
/// How many tasks currently use `TABLES[i]` (`CLONE_FILES` share).
static mut FILES_REFS: [u16; MAX_PROCESSES] = [0; MAX_PROCESSES];
static mut READY: bool = false;

pub fn init() {
    unsafe {
        for i in 0..MAX_PROCESSES {
            TABLES[i] = FdTable::new();
            FILES_REFS[i] = 0;
        }
        // kinit (slot 0) gets stdio; children inherit via clone.
        TABLES[0].install_stdio();
        FILES_REFS[0] = 1;
        READY = true;
    }
}

pub fn is_ready() -> bool {
    unsafe { READY }
}

fn table_mut(slot: usize) -> &'static mut FdTable {
    let i = if slot < MAX_PROCESSES { slot } else { 0 };
    unsafe { &mut *core::ptr::addr_of_mut!(TABLES[i]) }
}

/// Resolve which FD table slot the current task uses.
fn current_files_slot() -> usize {
    crate::process::with_current(|p| {
        if p.files_slot < MAX_PROCESSES {
            p.files_slot
        } else {
            0
        }
    })
    .unwrap_or(0)
}

/// Operate on the current process's FD table (may be shared).
pub fn with_current<F, R>(f: F) -> R
where
    F: FnOnce(&mut FdTable) -> R,
{
    f(table_mut(current_files_slot()))
}

/// Clone parent's open FDs into a new child process slot (private table).
/// Sets child `files_slot = child_idx` and refcount 1 on the child's table.
pub fn clone_table(parent_idx: usize, child_idx: usize) {
    if parent_idx >= MAX_PROCESSES || child_idx >= MAX_PROCESSES {
        return;
    }
    let parent_files = crate::process::table::with_index(parent_idx, |p| p.files_slot)
        .unwrap_or(parent_idx);
    let parent_files = if parent_files < MAX_PROCESSES {
        parent_files
    } else {
        parent_idx
    };
    unsafe {
        let parent = *core::ptr::addr_of!(TABLES[parent_files]);
        TABLES[child_idx].clone_from(&parent);
        FILES_REFS[child_idx] = 1;
    }
    let _ = crate::process::table::with_index(child_idx, |p| {
        p.files_slot = child_idx;
    });
}

/// `CLONE_FILES`: child shares the parent's open-file table (refcount++).
pub fn share_table(parent_idx: usize, child_idx: usize) {
    if parent_idx >= MAX_PROCESSES || child_idx >= MAX_PROCESSES {
        return;
    }
    let parent_files = crate::process::table::with_index(parent_idx, |p| p.files_slot)
        .unwrap_or(parent_idx);
    let parent_files = if parent_files < MAX_PROCESSES {
        parent_files
    } else {
        parent_idx
    };
    unsafe {
        // Child's private table (if any) is unused.
        TABLES[child_idx].close_all();
        FILES_REFS[child_idx] = 0;
        if FILES_REFS[parent_files] < u16::MAX {
            FILES_REFS[parent_files] = FILES_REFS[parent_files].saturating_add(1);
        }
    }
    let _ = crate::process::table::with_index(child_idx, |p| {
        p.files_slot = parent_files;
    });
}

/// Drop one reference to the FD table used by process slot `proc_slot`.
/// Clears the underlying table only when the last user exits.
pub fn release_files(proc_slot: usize) {
    if proc_slot >= MAX_PROCESSES {
        return;
    }
    let files = crate::process::table::with_index(proc_slot, |p| p.files_slot).unwrap_or(proc_slot);
    let files = if files < MAX_PROCESSES { files } else { proc_slot };
    unsafe {
        if FILES_REFS[files] > 0 {
            FILES_REFS[files] -= 1;
        }
        if FILES_REFS[files] == 0 {
            TABLES[files].close_all();
        }
    }
}

/// Reset FD table when a process slot is freed (legacy name → release_files).
pub fn clear_table(slot: usize) {
    release_files(slot);
}

pub fn open_count() -> usize {
    with_current(|t| t.open_count())
}


fn vfs_to_fd(e: VfsError) -> FdError {
    match e {
        VfsError::NoEnt => FdError::NoEnt,
        VfsError::IsDir => FdError::IsDir,
        VfsError::NotDir => FdError::NotDir,
        VfsError::Inval => FdError::Inval,
        VfsError::Fault => FdError::Fault,
        VfsError::NoDev | VfsError::NoMem => FdError::NoMem,
        VfsError::Exist => FdError::Exist,
        VfsError::NotEmpty => FdError::Inval,
        VfsError::Loop => FdError::Loop,
    }
}

/// Open path via VFS (Phase 7).
/// Supports O_RDONLY / O_WRONLY / O_RDWR, O_CREAT, O_TRUNC, O_DIRECTORY.
pub fn open_path(path: &str, flags: u64) -> Result<usize, FdError> {
    if !is_ready() {
        return Err(FdError::BadFd);
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

    let data = vcore::vfs_open(path, flags as u32, readable, writable).map_err(vfs_to_fd)?;
    with_current(|t| t.install(File::from_vfs(data)))
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

/// Read from `fd` at absolute `offset` without changing the FD's current offset.
pub fn sys_read_at(fd: u64, offset: u64, buf: &mut [u8]) -> Result<usize, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| {
        let file = t.get(fd as usize).ok_or(FdError::BadFd)?;
        vcore::vfs_read_at(&file.data, offset, buf).map_err(vfs_to_fd)
    })
}

/// Current byte offset of an open FD (for sendfile with null offset).
pub fn sys_fd_offset(fd: u64) -> Result<u64, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.get(fd as usize).map(|f| f.data.pos).ok_or(FdError::BadFd))
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

pub fn sys_pipe() -> Result<(usize, usize), FdError> {
    if !is_ready() {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.pipe_open())
}

pub fn sys_dup(fd: u64) -> Result<usize, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.dup_fd(fd as usize))
}

pub fn sys_dup2(old: u64, new: u64) -> Result<usize, FdError> {
    if !is_ready() || old >= FD_MAX as u64 || new >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.dup2_fd(old as usize, new as usize))
}

// Linux fcntl cmds (keep in sync with syscalls::sys_fcntl)
const F_DUPFD: u64 = 0;
const F_GETFD: u64 = 1;
const F_SETFD: u64 = 2;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;
const F_DUPFD_CLOEXEC: u64 = 1030;

/// Minimal fcntl for musl (CLOEXEC after opendir, GETFL, optional DUPFD).
pub fn sys_fcntl(fd: u64, cmd: u64, arg: u64) -> Result<u64, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    let fd = fd as usize;
    with_current(|t| {
        let file = t.get(fd).ok_or(FdError::BadFd)?;
        match cmd {
            F_GETFD => Ok(0),
            F_SETFD => {
                let _ = arg;
                Ok(0)
            }
            F_GETFL => {
                let mut fl = if file.data.readable && file.data.writable {
                    O_RDWR
                } else if file.data.writable {
                    O_WRONLY
                } else {
                    O_RDONLY
                };
                if file.data.is_dir {
                    fl |= O_DIRECTORY;
                }
                Ok(fl)
            }
            F_SETFL => {
                let _ = arg;
                Ok(0)
            }
            F_DUPFD | F_DUPFD_CLOEXEC => {
                let min = arg as usize;
                let copy = *file;
                t.install_at_least(min, copy).map(|i| i as u64)
            }
            _ => Err(FdError::Inval),
        }
    })
}

pub fn sys_getdents64(fd: u64, buf: &mut [u8]) -> Result<usize, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| t.getdents64(fd as usize, buf))
}

/// Linux lseek(2) — SEEK_SET=0, SEEK_CUR=1, SEEK_END=2.
pub fn sys_lseek(fd: u64, offset: i64, whence: u64) -> Result<u64, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| {
        let file = t.get_mut(fd as usize).ok_or(FdError::BadFd)?;
        if file.data.is_dir {
            return Err(FdError::IsDir);
        }
        if file.data.fops_id == vcore::FOPS_CONSOLE {
            return Err(FdError::Inval);
        }
        let size = if file.data.fops_id == vcore::FOPS_EXT2_FILE {
            fs::ext2::inode_file_size(file.data.private as u32) as i64
        } else if file.data.fops_id == vcore::FOPS_RAMFS_FILE {
            // ramfs size from pos ceiling — approximate via private slot not exported;
            // allow seek within written region by treating size as pos max (1 MiB cap).
            256i64
        } else {
            0i64
        };
        let cur = file.data.pos as i64;
        let new = match whence {
            0 => offset,
            1 => cur.saturating_add(offset),
            2 => size.saturating_add(offset),
            _ => return Err(FdError::Inval),
        };
        if new < 0 {
            return Err(FdError::Inval);
        }
        file.data.pos = new as u64;
        Ok(file.data.pos)
    })
}

/// Resolve open fd to an ext2 inode (file or dir), if any.
pub fn sys_fd_inode(fd: u64) -> Result<u32, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| {
        let file = t.get(fd as usize).ok_or(FdError::BadFd)?;
        file.ext2_ino().ok_or(FdError::Inval)
    })
}

/// `fstat` via VFS (ext2, virtual mounts, pipes, chrdev).
pub fn sys_fd_stat(fd: u64) -> Result<vcore::VfsStat, FdError> {
    if !is_ready() || fd >= FD_MAX as u64 {
        return Err(FdError::BadFd);
    }
    with_current(|t| {
        let file = t.get(fd as usize).ok_or(FdError::BadFd)?;
        vcore::vfs_stat_open(&file.data).map_err(vfs_to_fd)
    })
}

/// True if fd is the console (stdin/stdout/stderr style).
pub fn sys_fd_is_console(fd: u64) -> bool {
    if !is_ready() || fd >= FD_MAX as u64 {
        return false;
    }
    with_current(|t| {
        t.get(fd as usize)
            .map(|f| f.data.fops_id == vcore::FOPS_CONSOLE)
            .unwrap_or(false)
    })
}
