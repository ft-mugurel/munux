//! Phase 7d — VFS path mutations (mkdir/unlink/rmdir/rename/link).
//!
//! Syscalls go through here so backends stay pluggable; only ext2 implements
//! mutations today (virtual mounts return EROFS / EINVAL).

use crate::fs::ext2;
use crate::fs::ext2_write;
use crate::fs::path;
use crate::fs::vcore::{self, VfsError};

fn cwd() -> u32 {
    path::cwd_inode()
}

fn refuse_virtual_mut(path: &str) -> Result<(), VfsError> {
    // Disallow mutating under /proc /dev /ram (and absolute virtual roots).
    if path == "/proc"
        || path.starts_with("/proc/")
        || path == "/dev"
        || path.starts_with("/dev/")
        || path == "/ram"
        || path.starts_with("/ram/")
    {
        return Err(VfsError::Inval);
    }
    if vcore::is_virtual_ino(cwd()) {
        // relative path while cwd is virtual
        return Err(VfsError::Inval);
    }
    Ok(())
}

pub fn vfs_mkdir(path: &str) -> Result<(), VfsError> {
    refuse_virtual_mut(path)?;
    if !ext2::is_mounted() {
        return Err(VfsError::NoEnt);
    }
    match ext2_write::mkdir(cwd(), path) {
        Ok(_) => Ok(()),
        Err("exists") => Err(VfsError::Exist),
        Err("not a directory") => Err(VfsError::NotDir),
        Err(_) => Err(VfsError::Fault),
    }
}

pub fn vfs_unlink(path: &str) -> Result<(), VfsError> {
    refuse_virtual_mut(path)?;
    if !ext2::is_mounted() {
        return Err(VfsError::NoEnt);
    }
    match ext2_write::unlink(cwd(), path) {
        Ok(()) => Ok(()),
        Err("is a directory (use rmdir)") | Err("is a directory") => Err(VfsError::IsDir),
        Err("not found") | Err("no such") => Err(VfsError::NoEnt),
        Err(_) => Err(VfsError::Fault),
    }
}

pub fn vfs_rmdir(path: &str) -> Result<(), VfsError> {
    refuse_virtual_mut(path)?;
    if !ext2::is_mounted() {
        return Err(VfsError::NoEnt);
    }
    match ext2_write::rmdir(cwd(), path) {
        Ok(()) => Ok(()),
        Err("not a directory") => Err(VfsError::NotDir),
        Err("directory not empty") => Err(VfsError::NotEmpty),
        Err("not found") => Err(VfsError::NoEnt),
        Err(e) if e.contains("not empty") => Err(VfsError::NotEmpty),
        Err(_) => Err(VfsError::Fault),
    }
}

pub fn vfs_rename(old: &str, new: &str) -> Result<(), VfsError> {
    refuse_virtual_mut(old)?;
    refuse_virtual_mut(new)?;
    if !ext2::is_mounted() {
        return Err(VfsError::NoEnt);
    }
    match ext2_write::rename(cwd(), old, new) {
        Ok(()) => Ok(()),
        Err("exists") => Err(VfsError::Exist),
        Err("is a directory") => Err(VfsError::IsDir),
        Err("not a directory") => Err(VfsError::NotDir),
        Err("directory not empty") => Err(VfsError::NotEmpty),
        Err("not found") | Err("no such") => Err(VfsError::NoEnt),
        Err(_) => Err(VfsError::Fault),
    }
}

pub fn vfs_link(old: &str, new: &str) -> Result<(), VfsError> {
    refuse_virtual_mut(old)?;
    refuse_virtual_mut(new)?;
    if !ext2::is_mounted() {
        return Err(VfsError::NoEnt);
    }
    match ext2_write::link(cwd(), old, new) {
        Ok(()) => Ok(()),
        Err("exists") => Err(VfsError::Exist),
        Err("is a directory") => Err(VfsError::IsDir),
        Err("too many symlinks") => Err(VfsError::Loop),
        Err("not found") | Err("no such") => Err(VfsError::NoEnt),
        Err(_) => Err(VfsError::Fault),
    }
}

pub fn vfs_symlink(target: &str, linkpath: &str) -> Result<(), VfsError> {
    refuse_virtual_mut(linkpath)?;
    if !ext2::is_mounted() {
        return Err(VfsError::NoEnt);
    }
    match ext2_write::symlink(cwd(), target, linkpath) {
        Ok(()) => Ok(()),
        Err("exists") => Err(VfsError::Exist),
        Err("too many symlinks") => Err(VfsError::Loop),
        Err("not found") | Err("no such") => Err(VfsError::NoEnt),
        Err("not a directory") => Err(VfsError::NotDir),
        Err(_) => Err(VfsError::Fault),
    }
}

/// Map VfsError to a short string for existing map_fs_write_err.
pub fn vfs_err_str(e: VfsError) -> &'static str {
    match e {
        VfsError::NoEnt => "not found",
        VfsError::IsDir => "is a directory",
        VfsError::NotDir => "not a directory",
        VfsError::NotEmpty => "directory not empty",
        VfsError::Inval => "bad name",
        VfsError::Exist => "exists",
        VfsError::Loop => "too many symlinks",
        VfsError::Fault | VfsError::NoDev | VfsError::NoMem => "fault",
    }
}
