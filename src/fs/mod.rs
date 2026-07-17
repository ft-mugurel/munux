//! Filesystem: IDE probe + ext2 (read) + path helpers.

pub mod ext2;
pub mod path;
pub mod vfs;

use crate::console;
use crate::drivers::ide;

static mut FS_READY: bool = false;

/// Probe IDE and mount ext2 root if possible.
pub fn init() {
    console::print("fs: probing IDE... ");
    if !ide::init() {
        console::println("no disk");
        unsafe {
            FS_READY = false;
        }
        return;
    }
    console::print("OK sectors=");
    console::write_u64(ide::sector_count() as u64);
    console::println("");

    match ext2::mount() {
        Ok(()) => {
            unsafe {
                FS_READY = true;
            }
            path::set_cwd_inode(ext2::ROOT_INODE);
            console::print("fs: ext2 mounted root=");
            console::write_u64(ext2::ROOT_INODE as u64);
            console::println("");
            // Smoke: list root names on boot
            if let Ok(_) = ext2::list_dir(ext2::ROOT_INODE) {
                console::print("fs: root: ");
                for i in 0..vfs::cache_len() {
                    if let Some(n) = vfs::cache_get(i) {
                        let name = n.name_str();
                        if name == "." || name == ".." {
                            continue;
                        }
                        console::print(name);
                        console::print(" ");
                    }
                }
                console::println("");
            }
        }
        Err(e) => {
            console::print("fs: ext2 mount failed: ");
            console::println(e);
            unsafe {
                FS_READY = false;
            }
        }
    }
}

pub fn is_ready() -> bool {
    unsafe { FS_READY }
}
