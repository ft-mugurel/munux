//! Linux `file_operations` / `misc_register` / `register_chrdev`.

use crate::fs::vcore;
use crate::module;

/// Matches `include/linux/fs.h` `struct file`.
#[repr(C)]
pub struct LinuxFile {
    pub f_pos: i64,
    pub private_data: *mut u8,
    pub f_op: *const LinuxFileOperations,
    pub f_flags: u32,
}

/// Matches `include/linux/fs.h` `struct inode`.
#[repr(C)]
pub struct LinuxInode {
    pub i_rdev: u32,
}

/// Matches `include/linux/fs.h` `struct file_operations`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxFileOperations {
    pub owner: *mut u8,
    pub llseek: Option<extern "C" fn(*mut LinuxFile, i64, i32) -> i64>,
    pub read: Option<extern "C" fn(*mut LinuxFile, *mut u8, u64, *mut i64) -> i64>,
    pub write: Option<extern "C" fn(*mut LinuxFile, *const u8, u64, *mut i64) -> i64>,
    pub open: Option<extern "C" fn(*mut LinuxInode, *mut LinuxFile) -> i32>,
    pub release: Option<extern "C" fn(*mut LinuxInode, *mut LinuxFile) -> i32>,
}

/// Matches `include/linux/miscdevice.h` `struct miscdevice`.
#[repr(C)]
pub struct MiscDevice {
    pub minor: i32,
    pub name: *const u8,
    pub fops: *const LinuxFileOperations,
}

fn cstr_name<'a>(p: *const u8) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    let mut n = 0usize;
    while n < 32 {
        let b = unsafe { core::ptr::read_volatile(p.add(n)) };
        if b == 0 {
            break;
        }
        n += 1;
    }
    if n == 0 {
        return None;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, n) }).ok()
}

fn map_vfs(r: Result<(), vcore::VfsError>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(vcore::VfsError::Exist) => -17,
        Err(vcore::VfsError::NoMem) => -12,
        Err(vcore::VfsError::NoEnt) => -2,
        Err(vcore::VfsError::Inval) => -16, // EBUSY if still open
        _ => -22,
    }
}

/// `int register_chrdev(unsigned int major, const char *name, const struct file_operations *fops)`
pub extern "C" fn register_chrdev(
    _major: u32,
    name: *const u8,
    fops: *const LinuxFileOperations,
) -> i32 {
    let Some(n) = cstr_name(name) else {
        return -22;
    };
    if fops.is_null() {
        return -22;
    }
    let owner = module::loading_slot();
    map_vfs(vcore::register_linux_chrdev(n, fops, owner))
}

/// `int unregister_chrdev(unsigned int major, const char *name)`
pub extern "C" fn unregister_chrdev(_major: u32, name: *const u8) -> i32 {
    let Some(n) = cstr_name(name) else {
        return -22;
    };
    map_vfs(vcore::unregister_chrdev(n))
}

/// `int misc_register(struct miscdevice *misc)`
pub extern "C" fn misc_register(misc: *mut MiscDevice) -> i32 {
    if misc.is_null() {
        return -22;
    }
    let m = unsafe { &*misc };
    register_chrdev(0, m.name, m.fops)
}

/// `int misc_deregister(struct miscdevice *misc)`
pub extern "C" fn misc_deregister(misc: *mut MiscDevice) -> i32 {
    if misc.is_null() {
        return -22;
    }
    let m = unsafe { &*misc };
    unregister_chrdev(0, m.name)
}

pub unsafe fn call_read(
    fops: *const LinuxFileOperations,
    buf: *mut u8,
    len: u64,
    pos: &mut i64,
) -> Option<i64> {
    if fops.is_null() {
        return None;
    }
    let ops = &*fops;
    let read = ops.read?;
    let mut file = LinuxFile {
        f_pos: *pos,
        private_data: core::ptr::null_mut(),
        f_op: fops,
        f_flags: 0,
    };
    let rc = read(&mut file, buf, len, &mut file.f_pos);
    *pos = file.f_pos;
    Some(rc)
}

pub unsafe fn call_write(
    fops: *const LinuxFileOperations,
    buf: *const u8,
    len: u64,
    pos: &mut i64,
) -> Option<i64> {
    if fops.is_null() {
        return None;
    }
    let ops = &*fops;
    let write = ops.write?;
    let mut file = LinuxFile {
        f_pos: *pos,
        private_data: core::ptr::null_mut(),
        f_op: fops,
        f_flags: 0,
    };
    let rc = write(&mut file, buf, len, &mut file.f_pos);
    *pos = file.f_pos;
    Some(rc)
}

pub unsafe fn call_open(fops: *const LinuxFileOperations) -> i32 {
    if fops.is_null() {
        return 0;
    }
    let ops = &*fops;
    let Some(open) = ops.open else {
        return 0;
    };
    let mut inode = LinuxInode { i_rdev: 0 };
    let mut file = LinuxFile {
        f_pos: 0,
        private_data: core::ptr::null_mut(),
        f_op: fops,
        f_flags: 0,
    };
    open(&mut inode, &mut file)
}

pub unsafe fn call_release(fops: *const LinuxFileOperations) {
    if fops.is_null() {
        return;
    }
    let ops = &*fops;
    if let Some(release) = ops.release {
        let mut inode = LinuxInode { i_rdev: 0 };
        let mut file = LinuxFile {
            f_pos: 0,
            private_data: core::ptr::null_mut(),
            f_op: fops,
            f_flags: 0,
        };
        let _ = release(&mut inode, &mut file);
    }
}
