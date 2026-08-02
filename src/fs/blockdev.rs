//! Phase 7b — block device registration (ops tables for modules later).
//!
//! IDE primary master is registered as `hda`. Filesystems should use
//! [`read_sectors`] / [`write_sectors`] on a device name rather than calling
//! the IDE driver directly.

use crate::console;
use crate::drivers::ide;

pub const MAX_BLKDEV: usize = 4;

pub type BlkReadFn = fn(lba: u32, count: u32, buf: &mut [u8]) -> Result<(), &'static str>;
pub type BlkWriteFn = fn(lba: u32, count: u32, buf: &[u8]) -> Result<(), &'static str>;

#[derive(Clone, Copy)]
pub struct BlockDevice {
    pub used: bool,
    pub name: [u8; 8],
    pub name_len: u8,
    pub sector_size: u32,
    pub nsectors: u32,
    pub read: Option<BlkReadFn>,
    pub write: Option<BlkWriteFn>,
}

static mut DEVS: [BlockDevice; MAX_BLKDEV] = [BlockDevice {
    used: false,
    name: [0; 8],
    name_len: 0,
    sector_size: 512,
    nsectors: 0,
    read: None,
    write: None,
}; MAX_BLKDEV];

/// Default block device used by the root filesystem (`hda`).
static mut DEFAULT_DEV: i8 = -1;

fn devs_mut() -> &'static mut [BlockDevice; MAX_BLKDEV] {
    unsafe { &mut *core::ptr::addr_of_mut!(DEVS) }
}

fn name_eq(e: &BlockDevice, name: &str) -> bool {
    let n = e.name_len as usize;
    core::str::from_utf8(&e.name[..n]).unwrap_or("") == name
}

/// Register a block device. Returns slot index.
pub fn register_blkdev(
    name: &str,
    sector_size: u32,
    nsectors: u32,
    read: BlkReadFn,
    write: BlkWriteFn,
) -> Result<usize, &'static str> {
    let d = devs_mut();
    for (i, e) in d.iter_mut().enumerate() {
        if !e.used {
            e.used = true;
            e.name = [0; 8];
            let b = name.as_bytes();
            let n = b.len().min(7);
            e.name[..n].copy_from_slice(&b[..n]);
            e.name_len = n as u8;
            e.sector_size = sector_size;
            e.nsectors = nsectors;
            e.read = Some(read);
            e.write = Some(write);
            if unsafe { DEFAULT_DEV } < 0 {
                unsafe {
                    DEFAULT_DEV = i as i8;
                }
            }
            return Ok(i);
        }
    }
    Err("too many block devices")
}

pub fn set_default(name: &str) -> bool {
    for (i, e) in devs_mut().iter().enumerate() {
        if e.used && name_eq(e, name) {
            unsafe {
                DEFAULT_DEV = i as i8;
            }
            return true;
        }
    }
    false
}

fn default_slot() -> Option<usize> {
    let i = unsafe { DEFAULT_DEV };
    if i >= 0 {
        Some(i as usize)
    } else {
        None
    }
}

/// Read sectors from the default block device (root disk).
pub fn read_sectors(lba: u32, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    let i = default_slot().ok_or("no block device")?;
    let e = &devs_mut()[i];
    let op = e.read.ok_or("no read op")?;
    op(lba, count, buf)
}

/// Write sectors to the default block device.
pub fn write_sectors(lba: u32, count: u32, buf: &[u8]) -> Result<(), &'static str> {
    let i = default_slot().ok_or("no block device")?;
    let e = &devs_mut()[i];
    let op = e.write.ok_or("no write op")?;
    op(lba, count, buf)
}

pub fn read_sector(lba: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    read_sectors(lba, 1, buf)
}

pub fn write_sector(lba: u32, buf: &[u8]) -> Result<(), &'static str> {
    write_sectors(lba, 1, buf)
}

pub fn sector_count() -> u32 {
    default_slot()
        .map(|i| devs_mut()[i].nsectors)
        .unwrap_or(0)
}

pub fn sector_size() -> u32 {
    default_slot()
        .map(|i| devs_mut()[i].sector_size)
        .unwrap_or(512)
}

pub fn count() -> usize {
    devs_mut().iter().filter(|e| e.used).count()
}

pub fn name_at(i: usize) -> Option<&'static str> {
    let d = devs_mut();
    if i >= MAX_BLKDEV || !d[i].used {
        return None;
    }
    // Return known static labels only (avoid dangling refs into mut).
    let n = core::str::from_utf8(&d[i].name[..d[i].name_len as usize]).unwrap_or("");
    match n {
        "hda" => Some("hda"),
        _ => Some("blk"),
    }
}

fn ide_read(lba: u32, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    ide::read_sectors(lba, count, buf)
}

fn ide_write(lba: u32, count: u32, buf: &[u8]) -> Result<(), &'static str> {
    ide::write_sectors(lba, count, buf)
}

/// After IDE probe succeeds, register `hda` as the default block device.
pub fn init_from_ide() {
    if !ide::is_present() {
        return;
    }
    match register_blkdev(
        "hda",
        ide::sector_size() as u32,
        ide::sector_count(),
        ide_read,
        ide_write,
    ) {
        Ok(_) => {
            console::print("blockdev: hda sectors=");
            console::write_u64(ide::sector_count() as u64);
            console::println("");
        }
        Err(e) => {
            console::print("blockdev: register failed: ");
            console::println(e);
        }
    }
}
