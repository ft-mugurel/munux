//! Filesystem stack: VFS nodes + ext2 reader/writer.

pub mod ext2;
pub mod ext2_write;
pub mod path;
pub mod vfs;

use crate::drivers::ide;
use crate::process;

static mut FS_READY: bool = false;

/// Probe IDE and mount ext2 root if possible.
pub fn init() {
    crate::println!("fs: probing IDE...");
    if !ide::init() {
        crate::println!("fs: no IDE disk (attach -hda build/disk.img)");
        unsafe {
            FS_READY = false;
        }
        return;
    }
    crate::println!(
        "fs: IDE present, {} sectors ({} KiB)",
        ide::sector_count(),
        (ide::sector_count() as u64 * 512) / 1024
    );

    match ext2::mount() {
        Ok(()) => {
            unsafe {
                FS_READY = true;
            }
            process::set_cwd_inode(ext2::ROOT_INODE);
            crate::println!("fs: ext2 mounted, root inode={}", ext2::ROOT_INODE);
        }
        Err(e) => {
            crate::println!("fs: ext2 mount failed: {}", e);
            unsafe {
                FS_READY = false;
            }
        }
    }
}

pub fn is_ready() -> bool {
    unsafe { FS_READY }
}
