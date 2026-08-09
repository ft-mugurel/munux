//! linuxkpi disk registration → munux `/dev/<name>` + blockdev table.

use crate::fs::{blockdev, vcore};

pub type ModBlkRwFn = blockdev::ModBlkRwFn;

fn cstr_name<'a>(p: *const u8) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    let mut n = 0usize;
    while n < 12 {
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

/// `int munux_add_disk(name, nsectors, read, write)` — 0 ok, -errno on fail.
pub extern "C" fn munux_add_disk(
    name: *const u8,
    nsectors: u32,
    read: Option<ModBlkRwFn>,
    write: Option<ModBlkRwFn>,
) -> i32 {
    let Some(n) = cstr_name(name) else {
        return -22;
    };
    let (Some(r), Some(w)) = (read, write) else {
        return -22;
    };
    if nsectors == 0 {
        return -22;
    }
    match blockdev::register_mod_blkdev(n, nsectors, r, w) {
        Ok(_) => {
            let _ = vcore::register_chrdev(n, vcore::FOPS_BLK);
            0
        }
        Err(_) => -12,
    }
}

pub extern "C" fn munux_del_disk(name: *const u8) -> i32 {
    let Some(n) = cstr_name(name) else {
        return -22;
    };
    let _ = vcore::unregister_chrdev(n);
    if blockdev::unregister_blkdev(n) {
        0
    } else {
        -2
    }
}
