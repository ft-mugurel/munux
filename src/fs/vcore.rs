//! Phase 7a — VFS core: file_operations, mounts, char devices.
//!
//! Goal: open/read/write dispatch through **ops tables**, not ad-hoc `ext2_*`
//! calls in the FD layer. A second backend (ramfs + `/dev/null`) proves
//! registration works without rewriting syscalls.

use crate::console;
use crate::fs::ext2;
use crate::fs::ext2_write;
use crate::fs::path;
use crate::interrupts::keyboard::init as kbd;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsError {
    NoEnt,
    IsDir,
    NotDir,
    Inval,
    Fault,
    NoDev,
    Exist,
    NoMem,
}

// ---------------------------------------------------------------------------
// file_operations (Linux-shaped, static function pointers for module-ready ABI)
// ---------------------------------------------------------------------------

/// Open-file state passed to ops (like a slim `struct file`).
#[derive(Clone, Copy)]
pub struct FileData {
    /// Byte offset / dir cookie.
    pub pos: u64,
    pub readable: bool,
    pub writable: bool,
    /// Backend-private (ext2 ino, ramfs slot, …).
    pub private: u64,
    pub is_dir: bool,
    /// Index into [`FOPS_TABLE`] (or special ids).
    pub fops_id: u8,
}

impl FileData {
    pub const fn closed() -> Self {
        Self {
            pos: 0,
            readable: false,
            writable: false,
            private: 0,
            is_dir: false,
            fops_id: FOPS_NONE,
        }
    }
}

pub type ReadOp = fn(f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError>;
pub type WriteOp = fn(f: &mut FileData, buf: &[u8]) -> Result<usize, VfsError>;
pub type ReleaseOp = fn(f: &mut FileData);

pub struct FileOperations {
    pub name: &'static str,
    pub read: Option<ReadOp>,
    pub write: Option<WriteOp>,
    pub release: Option<ReleaseOp>,
}

// Stable fops ids (indices into FOPS_TABLE).
pub const FOPS_NONE: u8 = 0;
pub const FOPS_CONSOLE: u8 = 1;
pub const FOPS_EXT2_FILE: u8 = 2;
pub const FOPS_EXT2_DIR: u8 = 3;
pub const FOPS_RAMFS_FILE: u8 = 4;
pub const FOPS_NULL: u8 = 5;
pub const FOPS_ZERO: u8 = 6;
pub const FOPS_PROC: u8 = 7;
/// Virtual directory (mount point or synthetic folder).
pub const FOPS_VDIR: u8 = 8;

/// Synthetic inodes for virtual directories (not on ext2).
pub const VINO_PROC: u32 = 0xF000_0001;
pub const VINO_DEV: u32 = 0xF000_0002;
pub const VINO_RAM: u32 = 0xF000_0003;
pub const VINO_PROC_SELF: u32 = 0xF000_0004;

pub fn is_virtual_ino(ino: u32) -> bool {
    (ino & 0xF000_0000) == 0xF000_0000
}

// getdents cookie phases for root (inject mount-point names).
const COOKIE_VIRT: u64 = 0x8000_0000;
const COOKIE_EXT2_BEGIN: u64 = 0x4000_0000;

/// Linux dirent d_type
pub const DT_DIR: u8 = 4;
pub const DT_REG: u8 = 8;
pub const DT_CHR: u8 = 2;

static FOPS_TABLE: [FileOperations; 9] = [
    FileOperations {
        name: "none",
        read: None,
        write: None,
        release: None,
    },
    FileOperations {
        name: "console",
        read: Some(console_read_op),
        write: Some(console_write_op),
        release: None,
    },
    FileOperations {
        name: "ext2_file",
        read: Some(ext2_file_read_op),
        write: Some(ext2_file_write_op),
        release: None,
    },
    FileOperations {
        name: "ext2_dir",
        read: None,
        write: None,
        release: None,
    },
    FileOperations {
        name: "ramfs_file",
        read: Some(ramfs_read_op),
        write: Some(ramfs_write_op),
        release: None,
    },
    FileOperations {
        name: "null",
        read: Some(null_read_op),
        write: Some(null_write_op),
        release: None,
    },
    FileOperations {
        name: "zero",
        read: Some(zero_read_op),
        write: Some(null_write_op),
        release: None,
    },
    FileOperations {
        name: "proc",
        read: Some(proc_read_op),
        write: None,
        release: None,
    },
    FileOperations {
        name: "vdir",
        read: None,
        write: None,
        release: None,
    },
];

pub fn fops_name(id: u8) -> &'static str {
    let i = id as usize;
    if i < FOPS_TABLE.len() {
        FOPS_TABLE[i].name
    } else {
        "?"
    }
}

pub fn vfs_read(f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError> {
    if !f.readable {
        return Err(VfsError::Inval);
    }
    let id = f.fops_id as usize;
    if id >= FOPS_TABLE.len() {
        return Err(VfsError::NoDev);
    }
    match FOPS_TABLE[id].read {
        Some(op) => {
            let n = op(f, buf)?;
            f.pos = f.pos.saturating_add(n as u64);
            Ok(n)
        }
        None => Err(VfsError::IsDir),
    }
}

pub fn vfs_write(f: &mut FileData, buf: &[u8]) -> Result<usize, VfsError> {
    if !f.writable {
        return Err(VfsError::Inval);
    }
    let id = f.fops_id as usize;
    if id >= FOPS_TABLE.len() {
        return Err(VfsError::NoDev);
    }
    match FOPS_TABLE[id].write {
        Some(op) => {
            let n = op(f, buf)?;
            f.pos = f.pos.saturating_add(n as u64);
            Ok(n)
        }
        None => Err(VfsError::IsDir),
    }
}

pub fn vfs_read_at(f: &FileData, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
    if !f.readable {
        return Err(VfsError::Inval);
    }
    let mut tmp = *f;
    tmp.pos = offset;
    let id = tmp.fops_id as usize;
    if id >= FOPS_TABLE.len() {
        return Err(VfsError::NoDev);
    }
    match FOPS_TABLE[id].read {
        Some(op) => op(&mut tmp, buf),
        None => Err(VfsError::IsDir),
    }
}

// ---------------------------------------------------------------------------
// Mount table + char devices
// ---------------------------------------------------------------------------

pub const MAX_MOUNTS: usize = 4;
pub const MAX_CHRDEV: usize = 8;

#[derive(Clone, Copy)]
pub struct MountEntry {
    pub used: bool,
    /// Mount path prefix without trailing slash ("" = root, "/ram", "/dev").
    pub path: [u8; 16],
    pub path_len: u8,
    pub fs_name: &'static str,
    /// Root inode for this mount (ext2: 2, ramfs: 0, …).
    pub root_ino: u32,
}

#[derive(Clone, Copy)]
pub struct ChrdevEntry {
    pub used: bool,
    pub name: [u8; 12],
    pub name_len: u8,
    pub fops_id: u8,
}

static mut MOUNTS: [MountEntry; MAX_MOUNTS] = [MountEntry {
    used: false,
    path: [0; 16],
    path_len: 0,
    fs_name: "",
    root_ino: 0,
}; MAX_MOUNTS];

static mut CHRDEVS: [ChrdevEntry; MAX_CHRDEV] = [ChrdevEntry {
    used: false,
    name: [0; 12],
    name_len: 0,
    fops_id: FOPS_NONE,
}; MAX_CHRDEV];

static mut VFS_READY: bool = false;

fn mounts_mut() -> &'static mut [MountEntry; MAX_MOUNTS] {
    unsafe { &mut *core::ptr::addr_of_mut!(MOUNTS) }
}

fn chrdevs_mut() -> &'static mut [ChrdevEntry; MAX_CHRDEV] {
    unsafe { &mut *core::ptr::addr_of_mut!(CHRDEVS) }
}

/// Register a mount at `path` (e.g. `""` for `/`, `"/ram"`).
pub fn register_mount(path: &str, fs_name: &'static str, root_ino: u32) -> Result<(), VfsError> {
    let m = mounts_mut();
    for e in m.iter_mut() {
        if !e.used {
            e.used = true;
            e.path = [0; 16];
            let bytes = path.as_bytes();
            let n = bytes.len().min(15);
            e.path[..n].copy_from_slice(&bytes[..n]);
            e.path_len = n as u8;
            e.fs_name = fs_name;
            e.root_ino = root_ino;
            return Ok(());
        }
    }
    Err(VfsError::NoMem)
}

/// Register a character device name under `/dev/<name>`.
pub fn register_chrdev(name: &str, fops_id: u8) -> Result<(), VfsError> {
    let c = chrdevs_mut();
    for e in c.iter_mut() {
        if !e.used {
            e.used = true;
            e.name = [0; 12];
            let bytes = name.as_bytes();
            let n = bytes.len().min(11);
            e.name[..n].copy_from_slice(&bytes[..n]);
            e.name_len = n as u8;
            e.fops_id = fops_id;
            return Ok(());
        }
    }
    Err(VfsError::NoMem)
}

fn mount_path_str(e: &MountEntry) -> &str {
    core::str::from_utf8(&e.path[..e.path_len as usize]).unwrap_or("")
}

fn chrdev_name_str(e: &ChrdevEntry) -> &str {
    core::str::from_utf8(&e.name[..e.name_len as usize]).unwrap_or("")
}

/// Boot-time VFS setup (call after ext2 mount succeeds).
pub fn init_after_ext2() {
    let _ = register_mount("", "ext2", ext2::ROOT_INODE);
    let _ = register_mount("/ram", "ramfs", 0);
    let _ = register_mount("/proc", "proc", 0);
    let _ = register_chrdev("null", FOPS_NULL);
    let _ = register_chrdev("zero", FOPS_ZERO);
    ramfs_init();
    unsafe {
        VFS_READY = true;
    }
    console::print("vfs: mounts=");
    let mut n = 0u64;
    for e in mounts_mut().iter() {
        if e.used {
            n += 1;
        }
    }
    console::write_u64(n);
    console::print(" chrdev=");
    n = 0;
    for e in chrdevs_mut().iter() {
        if e.used {
            n += 1;
        }
    }
    console::write_u64(n);
    console::print(" blkdev=");
    console::write_u64(crate::fs::blockdev::count() as u64);
    console::println("");
}

pub fn is_ready() -> bool {
    unsafe { VFS_READY }
}

/// Debug: how many mounts / chrdevs registered.
pub fn stats() -> (usize, usize) {
    let mut m = 0usize;
    let mut c = 0usize;
    for e in mounts_mut().iter() {
        if e.used {
            m += 1;
        }
    }
    for e in chrdevs_mut().iter() {
        if e.used {
            c += 1;
        }
    }
    (m, c)
}

pub fn mount_name_at(i: usize) -> Option<(&'static str, &'static str)> {
    let m = mounts_mut();
    if i >= MAX_MOUNTS || !m[i].used {
        return None;
    }
    // path string is in static mut — return fs_name and a static path label
    let path = match mount_path_str(&m[i]) {
        "" => "/",
        "/ram" => "/ram",
        "/proc" => "/proc",
        _ => "?",
    };
    Some((path, m[i].fs_name))
}

// ---------------------------------------------------------------------------
// Path open
// ---------------------------------------------------------------------------

/// Open a path relative to process cwd (absolute if starts with `/`).
pub fn vfs_open(path: &str, flags: u32, readable: bool, writable: bool) -> Result<FileData, VfsError> {
    if path.is_empty() {
        return Err(VfsError::NoEnt);
    }
    const O_DIRECTORY: u32 = 0o200000;

    // Relative open while cwd is a virtual directory.
    let cwd = path::cwd_inode();
    if !path.starts_with('/') && is_virtual_ino(cwd) {
        return open_under_vdir(cwd, path, flags, readable, writable);
    }

    // Directory open for mount points (Linux: these show up under /).
    if path == "/proc" || path == "/proc/" {
        if writable {
            return Err(VfsError::IsDir);
        }
        return Ok(vdir_file(VINO_PROC));
    }
    if path == "/dev" || path == "/dev/" {
        if writable {
            return Err(VfsError::IsDir);
        }
        return Ok(vdir_file(VINO_DEV));
    }
    if path == "/ram" || path == "/ram/" {
        if writable {
            return Err(VfsError::IsDir);
        }
        return Ok(vdir_file(VINO_RAM));
    }
    if path == "/proc/self" || path == "/proc/self/" {
        if writable {
            return Err(VfsError::IsDir);
        }
        return Ok(vdir_file(VINO_PROC_SELF));
    }

    // Virtual /dev nodes (chrdev registry).
    if let Some(name) = strip_dev_prefix(path) {
        return open_chrdev(name, readable, writable);
    }

    // /proc/... → procfs files
    if path.starts_with("/proc/") || path.starts_with("proc/") {
        return crate::fs::procfs::open(path, readable, writable);
    }

    // /ram/... → ramfs
    if path.starts_with("/ram/") {
        return ramfs_open(path, flags, readable, writable);
    }

    // Relative names that match mount points when cwd is root
    if cwd == ext2::ROOT_INODE || cwd == 0 {
        match path {
            "proc" | "./proc" => {
                if writable {
                    return Err(VfsError::IsDir);
                }
                return Ok(vdir_file(VINO_PROC));
            }
            "dev" | "./dev" => {
                if writable {
                    return Err(VfsError::IsDir);
                }
                return Ok(vdir_file(VINO_DEV));
            }
            "ram" | "./ram" => {
                if writable {
                    return Err(VfsError::IsDir);
                }
                return Ok(vdir_file(VINO_RAM));
            }
            _ => {}
        }
    }

    // Default: ext2 root mount
    let f = ext2_open(path, flags, readable, writable)?;
    if (flags & O_DIRECTORY) != 0 && !f.is_dir {
        return Err(VfsError::NotDir);
    }
    Ok(f)
}

fn vdir_file(vino: u32) -> FileData {
    FileData {
        pos: 0,
        readable: true,
        writable: false,
        private: vino as u64,
        is_dir: true,
        fops_id: FOPS_VDIR,
    }
}

fn open_under_vdir(
    vino: u32,
    name: &str,
    flags: u32,
    readable: bool,
    writable: bool,
) -> Result<FileData, VfsError> {
    // strip ./
    let name = name.strip_prefix("./").unwrap_or(name);
    if name.is_empty() || name == "." {
        return Ok(vdir_file(vino));
    }
    if name == ".." {
        // parent of mount points is root
        if vino == VINO_PROC_SELF {
            return Ok(vdir_file(VINO_PROC));
        }
        return ext2_open("/", flags, true, false);
    }
    match vino {
        VINO_PROC => {
            if name == "self" {
                return Ok(vdir_file(VINO_PROC_SELF));
            }
            crate::fs::procfs::open_name(name, readable, writable)
        }
        VINO_PROC_SELF => {
            if name == "status" {
                crate::fs::procfs::open("/proc/self/status", readable, writable)
            } else {
                Err(VfsError::NoEnt)
            }
        }
        VINO_DEV => open_chrdev(name, readable, writable),
        VINO_RAM => ramfs_open_name(name, flags, readable, writable),
        _ => Err(VfsError::NoEnt),
    }
}

// ---------------------------------------------------------------------------
// Directory listing (getdents)
// ---------------------------------------------------------------------------

/// One directory entry for getdents64 packing.
pub struct VfsDirEnt {
    pub ino: u64,
    pub next_off: u64,
    pub d_type: u8,
    pub name: [u8; 32],
    pub name_len: u8,
}

fn make_dent(ino: u64, next_off: u64, d_type: u8, name: &str) -> VfsDirEnt {
    let mut d = VfsDirEnt {
        ino,
        next_off,
        d_type,
        name: [0; 32],
        name_len: 0,
    };
    let b = name.as_bytes();
    let n = b.len().min(31);
    d.name[..n].copy_from_slice(&b[..n]);
    d.name_len = n as u8;
    d
}

/// Next dirent for an open directory `f` at cookie `pos`.
/// Returns `None` when the listing is finished.
pub fn vfs_dir_next(f: &FileData, pos: u64) -> Result<Option<VfsDirEnt>, VfsError> {
    if f.fops_id == FOPS_VDIR {
        return Ok(vdir_next(f.private as u32, pos));
    }
    if f.fops_id == FOPS_EXT2_DIR {
        let ino = f.private as u32;
        // Root: inject mount-point names so `ls /` matches Linux.
        if ino == ext2::ROOT_INODE {
            return root_dir_next(pos);
        }
        return ext2_dir_next(ino, pos);
    }
    Err(VfsError::NotDir)
}

fn root_dir_next(pos: u64) -> Result<Option<VfsDirEnt>, VfsError> {
    // Phase 1: virtual mount points
    // pos==0 → first virt; COOKIE_VIRT|i → virt i; COOKIE_EXT2_BEGIN → start ext2
    const VNAMES: &[&str] = &["proc", "dev", "ram"];
    const VINOS: &[u32] = &[VINO_PROC, VINO_DEV, VINO_RAM];

    if pos == 0 || (pos & COOKIE_VIRT) != 0 {
        let idx = if pos == 0 {
            0u64
        } else {
            pos & !COOKIE_VIRT
        };
        if (idx as usize) < VNAMES.len() {
            let i = idx as usize;
            let next = if i + 1 < VNAMES.len() {
                COOKIE_VIRT | ((i as u64) + 1)
            } else {
                COOKIE_EXT2_BEGIN
            };
            return Ok(Some(make_dent(
                VINOS[i] as u64,
                next,
                DT_DIR,
                VNAMES[i],
            )));
        }
    }

    let ext2_cookie = if pos == COOKIE_EXT2_BEGIN {
        0
    } else if pos & COOKIE_VIRT == 0 && pos != 0 {
        pos
    } else if pos == 0 {
        // should have been handled
        0
    } else {
        return Ok(None);
    };

    // Skip ext2 entries that collide with our virtual names (if any).
    let mut cookie = ext2_cookie as u32;
    loop {
        match ext2::dir_next_entry(ext2::ROOT_INODE, cookie) {
            Ok(None) => return Ok(None),
            Err(_) => return Err(VfsError::Fault),
            Ok(Some(e)) => {
                let name = core::str::from_utf8(&e.name[..e.name_len as usize]).unwrap_or("");
                cookie = e.next_off;
                if name == "proc" || name == "dev" || name == "ram" {
                    continue; // prefer virtual mounts
                }
                let d_type = e.d_type;
                return Ok(Some(make_dent(
                    e.ino as u64,
                    e.next_off as u64,
                    d_type,
                    name,
                )));
            }
        }
    }
}

fn ext2_dir_next(ino: u32, pos: u64) -> Result<Option<VfsDirEnt>, VfsError> {
    match ext2::dir_next_entry(ino, pos as u32) {
        Ok(None) => Ok(None),
        Err(_) => Err(VfsError::Fault),
        Ok(Some(e)) => {
            let name = core::str::from_utf8(&e.name[..e.name_len as usize]).unwrap_or("");
            Ok(Some(make_dent(
                e.ino as u64,
                e.next_off as u64,
                e.d_type,
                name,
            )))
        }
    }
}

fn vdir_next(vino: u32, pos: u64) -> Option<VfsDirEnt> {
    // pos is sequential index 0,1,2,...
    let idx = pos as usize;
    match vino {
        VINO_PROC => {
            const NAMES: &[&str] = &[".", "..", "meminfo", "mounts", "version", "uptime", "self"];
            const TYPES: &[u8] = &[DT_DIR, DT_DIR, DT_REG, DT_REG, DT_REG, DT_REG, DT_DIR];
            const INOS: &[u64] = &[
                VINO_PROC as u64,
                ext2::ROOT_INODE as u64,
                0xF000_0101,
                0xF000_0102,
                0xF000_0103,
                0xF000_0104,
                VINO_PROC_SELF as u64,
            ];
            if idx >= NAMES.len() {
                return None;
            }
            Some(make_dent(
                INOS[idx],
                (idx as u64) + 1,
                TYPES[idx],
                NAMES[idx],
            ))
        }
        VINO_PROC_SELF => {
            const NAMES: &[&str] = &[".", "..", "status"];
            const TYPES: &[u8] = &[DT_DIR, DT_DIR, DT_REG];
            const INOS: &[u64] = &[
                VINO_PROC_SELF as u64,
                VINO_PROC as u64,
                0xF000_0105,
            ];
            if idx >= NAMES.len() {
                return None;
            }
            Some(make_dent(
                INOS[idx],
                (idx as u64) + 1,
                TYPES[idx],
                NAMES[idx],
            ))
        }
        VINO_DEV => {
            // ., .., then registered chrdevs
            if idx == 0 {
                return Some(make_dent(VINO_DEV as u64, 1, DT_DIR, "."));
            }
            if idx == 1 {
                return Some(make_dent(ext2::ROOT_INODE as u64, 2, DT_DIR, ".."));
            }
            let mut n = 0usize;
            for e in chrdevs_mut().iter() {
                if !e.used {
                    continue;
                }
                if n + 2 == idx {
                    let name = chrdev_name_str(e);
                    return Some(make_dent(
                        0xF000_0200 + n as u64,
                        (idx as u64) + 1,
                        DT_CHR,
                        name,
                    ));
                }
                n += 1;
            }
            None
        }
        VINO_RAM => {
            if idx == 0 {
                return Some(make_dent(VINO_RAM as u64, 1, DT_DIR, "."));
            }
            if idx == 1 {
                return Some(make_dent(ext2::ROOT_INODE as u64, 2, DT_DIR, ".."));
            }
            let mut n = 0usize;
            let r = ramfs_mut();
            for slot in r.iter() {
                if !slot.used {
                    continue;
                }
                if n + 2 == idx {
                    let name =
                        core::str::from_utf8(&slot.name[..slot.name_len as usize]).unwrap_or("?");
                    return Some(make_dent(
                        0xF000_0300 + n as u64,
                        (idx as u64) + 1,
                        DT_REG,
                        name,
                    ));
                }
                n += 1;
            }
            None
        }
        _ => None,
    }
}

fn ramfs_open_name(
    name: &str,
    flags: u32,
    readable: bool,
    writable: bool,
) -> Result<FileData, VfsError> {
    // reuse path form
    let mut path_buf = [0u8; 32];
    let p = b"/ram/";
    if p.len() + name.len() >= path_buf.len() {
        return Err(VfsError::NoEnt);
    }
    path_buf[..p.len()].copy_from_slice(p);
    path_buf[p.len()..p.len() + name.len()].copy_from_slice(name.as_bytes());
    let path = core::str::from_utf8(&path_buf[..p.len() + name.len()]).unwrap_or("");
    ramfs_open(path, flags, readable, writable)
}

fn strip_dev_prefix(path: &str) -> Option<&str> {
    if let Some(rest) = path.strip_prefix("/dev/") {
        if !rest.is_empty() && !rest.contains('/') {
            return Some(rest);
        }
    }
    // relative "dev/null" from /
    if let Some(rest) = path.strip_prefix("dev/") {
        if !rest.is_empty() && !rest.contains('/') {
            return Some(rest);
        }
    }
    None
}

fn open_chrdev(name: &str, readable: bool, writable: bool) -> Result<FileData, VfsError> {
    for e in chrdevs_mut().iter() {
        if e.used && chrdev_name_str(e) == name {
            return Ok(FileData {
                pos: 0,
                readable,
                writable,
                private: 0,
                is_dir: false,
                fops_id: e.fops_id,
            });
        }
    }
    Err(VfsError::NoEnt)
}

fn ext2_open(path: &str, flags: u32, readable: bool, writable: bool) -> Result<FileData, VfsError> {
    if !crate::fs::is_ready() {
        return Err(VfsError::NoEnt);
    }
    let cwd = path::cwd_inode();
    const O_CREAT: u32 = 0o100;
    const O_TRUNC: u32 = 0o1000;
    const O_DIRECTORY: u32 = 0o200000;

    let ino = match ext2::resolve_path(cwd, path) {
        Ok(i) => i,
        Err(_) => {
            if flags & O_CREAT == 0 {
                return Err(VfsError::NoEnt);
            }
            ext2_write::touch(cwd, path).map_err(|_| VfsError::NoEnt)?;
            ext2::resolve_path(cwd, path).map_err(|_| VfsError::NoEnt)?
        }
    };
    let is_dir = ext2::inode_is_dir(ino);
    if flags & O_DIRECTORY != 0 && !is_dir {
        return Err(VfsError::NotDir);
    }
    if is_dir && writable {
        return Err(VfsError::IsDir);
    }
    if !is_dir && (flags & O_TRUNC) != 0 && writable {
        let _ = ext2_write::truncate_file(ino);
    }
    Ok(FileData {
        pos: 0,
        readable: if is_dir { true } else { readable },
        writable: if is_dir { false } else { writable },
        private: ino as u64,
        is_dir,
        fops_id: if is_dir { FOPS_EXT2_DIR } else { FOPS_EXT2_FILE },
    })
}

// ---------------------------------------------------------------------------
// Ops implementations
// ---------------------------------------------------------------------------

fn console_write_op(_f: &mut FileData, data: &[u8]) -> Result<usize, VfsError> {
    let mut n = 0usize;
    for &b in data {
        if b == b'\n' || b == b'\t' {
            console::put_char(b);
            n += 1;
        } else if b == 0x08 || b == 0x7F {
            console::put_char(0x08);
            n += 1;
        } else if b == 0x0C {
            console::clear();
            n += 1;
        } else if b == 0x0E {
            console::set_inverse(true);
            n += 1;
        } else if b == 0x0F {
            console::set_inverse(false);
            n += 1;
        } else if (0x20..=0xFF).contains(&b) && b != 0x7F {
            console::put_char(b);
            n += 1;
        }
    }
    Ok(n)
}

fn console_read_op(_f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError> {
    if buf.is_empty() {
        return Ok(0);
    }
    crate::tty::enter_console_read();
    loop {
        crate::tty::deliver_pending_sigint();
        if let Some(sig) = crate::tty::take_force_fatal() {
            crate::syscalls::fatal_signal_exit(sig);
        }
        if kbd::buffered_len() > 0 {
            break;
        }
        unsafe {
            core::arch::asm!("sti; hlt", options(nostack));
        }
        crate::tty::deliver_pending_sigint();
        if let Some(sig) = crate::tty::take_force_fatal() {
            crate::syscalls::fatal_signal_exit(sig);
        }
        if kbd::buffered_len() > 0 {
            break;
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
    crate::tty::leave_console_read();
    Ok(n)
}

fn ext2_file_read_op(f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError> {
    let ino = f.private as u32;
    if f.pos > u32::MAX as u64 {
        return Ok(0);
    }
    match ext2::read_file(ino, f.pos as u32, buf) {
        Ok(n) => Ok(n),
        Err(_) => Err(VfsError::Fault),
    }
}

fn ext2_file_write_op(f: &mut FileData, data: &[u8]) -> Result<usize, VfsError> {
    let ino = f.private as u32;
    if f.pos > u32::MAX as u64 {
        return Ok(0);
    }
    match ext2_write::write_file_at(ino, f.pos as u32, data) {
        Ok(n) => Ok(n),
        Err("file too large") => Err(VfsError::Inval),
        Err("is a directory") => Err(VfsError::IsDir),
        Err(_) => Err(VfsError::Fault),
    }
}

fn null_read_op(_f: &mut FileData, _buf: &mut [u8]) -> Result<usize, VfsError> {
    Ok(0)
}

fn null_write_op(_f: &mut FileData, data: &[u8]) -> Result<usize, VfsError> {
    Ok(data.len())
}

fn zero_read_op(_f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError> {
    for b in buf.iter_mut() {
        *b = 0;
    }
    Ok(buf.len())
}

fn proc_read_op(f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError> {
    crate::fs::procfs::read_op(f, buf)
}

// ---------------------------------------------------------------------------
// Tiny ramfs — second registered FS (Phase 7 exit criterion)
// ---------------------------------------------------------------------------

const RAMFS_SLOTS: usize = 4;
const RAMFS_BYTES: usize = 256;

struct RamFile {
    used: bool,
    name: [u8; 16],
    name_len: u8,
    len: u16,
    data: [u8; RAMFS_BYTES],
}

impl RamFile {
    const fn empty() -> Self {
        Self {
            used: false,
            name: [0; 16],
            name_len: 0,
            len: 0,
            data: [0; RAMFS_BYTES],
        }
    }
}

static mut RAMFS: [RamFile; RAMFS_SLOTS] = [
    RamFile::empty(),
    RamFile::empty(),
    RamFile::empty(),
    RamFile::empty(),
];

fn ramfs_mut() -> &'static mut [RamFile; RAMFS_SLOTS] {
    unsafe { &mut *core::ptr::addr_of_mut!(RAMFS) }
}

fn ramfs_init() {
    // Seed hello file
    let r = ramfs_mut();
    r[0].used = true;
    let hello = b"hello";
    r[0].name[..5].copy_from_slice(hello);
    r[0].name_len = 5;
    let msg = b"ramfs says hi\n";
    r[0].data[..msg.len()].copy_from_slice(msg);
    r[0].len = msg.len() as u16;
}

fn ramfs_open(path: &str, flags: u32, readable: bool, writable: bool) -> Result<FileData, VfsError> {
    const O_CREAT: u32 = 0o100;
    // path is /ram or /ram/NAME
    let name = if path == "/ram" || path == "/ram/" {
        return Err(VfsError::IsDir);
    } else if let Some(n) = path.strip_prefix("/ram/") {
        n
    } else {
        return Err(VfsError::NoEnt);
    };
    if name.is_empty() || name.contains('/') {
        return Err(VfsError::NoEnt);
    }

    // Find or create
    let r = ramfs_mut();
    for (i, slot) in r.iter().enumerate() {
        if slot.used {
            let sname = core::str::from_utf8(&slot.name[..slot.name_len as usize]).unwrap_or("");
            if sname == name {
                return Ok(FileData {
                    pos: 0,
                    readable,
                    writable,
                    private: i as u64,
                    is_dir: false,
                    fops_id: FOPS_RAMFS_FILE,
                });
            }
        }
    }
    if flags & O_CREAT == 0 {
        return Err(VfsError::NoEnt);
    }
    for (i, slot) in r.iter_mut().enumerate() {
        if !slot.used {
            slot.used = true;
            slot.name = [0; 16];
            let b = name.as_bytes();
            let n = b.len().min(15);
            slot.name[..n].copy_from_slice(&b[..n]);
            slot.name_len = n as u8;
            slot.len = 0;
            slot.data = [0; RAMFS_BYTES];
            return Ok(FileData {
                pos: 0,
                readable,
                writable,
                private: i as u64,
                is_dir: false,
                fops_id: FOPS_RAMFS_FILE,
            });
        }
    }
    Err(VfsError::NoMem)
}

fn ramfs_read_op(f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError> {
    let i = f.private as usize;
    let r = ramfs_mut();
    if i >= RAMFS_SLOTS || !r[i].used {
        return Err(VfsError::NoEnt);
    }
    let len = r[i].len as u64;
    if f.pos >= len {
        return Ok(0);
    }
    let start = f.pos as usize;
    let avail = (len as usize).saturating_sub(start);
    let n = avail.min(buf.len());
    buf[..n].copy_from_slice(&r[i].data[start..start + n]);
    Ok(n)
}

fn ramfs_write_op(f: &mut FileData, data: &[u8]) -> Result<usize, VfsError> {
    let i = f.private as usize;
    let r = ramfs_mut();
    if i >= RAMFS_SLOTS || !r[i].used {
        return Err(VfsError::NoEnt);
    }
    let start = f.pos as usize;
    if start >= RAMFS_BYTES {
        return Err(VfsError::Inval);
    }
    let n = data.len().min(RAMFS_BYTES - start);
    r[i].data[start..start + n].copy_from_slice(&data[..n]);
    let end = (start + n) as u16;
    if end > r[i].len {
        r[i].len = end;
    }
    Ok(n)
}

/// Console stdio handle for FD 0/1/2 install.
pub fn console_file(readable: bool, writable: bool) -> FileData {
    FileData {
        pos: 0,
        readable,
        writable,
        private: 0,
        is_dir: false,
        fops_id: FOPS_CONSOLE,
    }
}
