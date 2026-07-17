//! ATA/IDE PIO interface (primary bus, master drive).
//!
//! Ports (primary):
//!   0x1F0 data, 0x1F1 error, 0x1F2 sector count, 0x1F3 LBA low,
//!   0x1F4 LBA mid, 0x1F5 LBA high, 0x1F6 drive/head, 0x1F7 status/cmd
//!   0x3F6 alternate status / device control

use crate::x86::io::{inb, inw, outb, outw};

const ATA_PRIMARY_IO: u16 = 0x1F0;
const ATA_PRIMARY_CTRL: u16 = 0x3F6;

const ATA_REG_DATA: u16 = 0;
const ATA_REG_SECCOUNT: u16 = 2;
const ATA_REG_LBA0: u16 = 3;
const ATA_REG_LBA1: u16 = 4;
const ATA_REG_LBA2: u16 = 5;
const ATA_REG_DRIVE: u16 = 6;
const ATA_REG_STATUS: u16 = 7;
const ATA_REG_CMD: u16 = 7;

const ATA_SR_BSY: u8 = 0x80;
const ATA_SR_DRQ: u8 = 0x08;
const ATA_SR_ERR: u8 = 0x01;

const ATA_CMD_READ_PIO: u8 = 0x20;
const ATA_CMD_WRITE_PIO: u8 = 0x30;
const ATA_CMD_IDENTIFY: u8 = 0xEC;
const ATA_CMD_CACHE_FLUSH: u8 = 0xE7;

const SECTOR_SIZE: usize = 512;

static mut PRESENT: bool = false;
static mut TOTAL_SECTORS: u32 = 0;

#[inline]
unsafe fn status() -> u8 {
    inb(ATA_PRIMARY_IO + ATA_REG_STATUS)
}

unsafe fn wait_bsy() {
    for _ in 0..1_000_000 {
        if status() & ATA_SR_BSY == 0 {
            return;
        }
    }
}

unsafe fn wait_drq() -> bool {
    for _ in 0..1_000_000 {
        let s = status();
        if s & ATA_SR_ERR != 0 {
            return false;
        }
        if s & ATA_SR_DRQ != 0 {
            return true;
        }
    }
    false
}

unsafe fn io_delay() {
    let _ = inb(ATA_PRIMARY_CTRL);
    let _ = inb(ATA_PRIMARY_CTRL);
    let _ = inb(ATA_PRIMARY_CTRL);
    let _ = inb(ATA_PRIMARY_CTRL);
}

/// Probe primary master; fill identify info if present.
pub fn init() -> bool {
    unsafe {
        // Select master, LBA mode
        outb(ATA_PRIMARY_IO + ATA_REG_DRIVE, 0xE0);
        io_delay();
        outb(ATA_PRIMARY_IO + ATA_REG_SECCOUNT, 0);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA0, 0);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA1, 0);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA2, 0);
        outb(ATA_PRIMARY_IO + ATA_REG_CMD, ATA_CMD_IDENTIFY);
        io_delay();

        if status() == 0 {
            PRESENT = false;
            return false;
        }
        wait_bsy();
        // Not ATAPI
        let lba1 = inb(ATA_PRIMARY_IO + ATA_REG_LBA1);
        let lba2 = inb(ATA_PRIMARY_IO + ATA_REG_LBA2);
        if lba1 != 0 || lba2 != 0 {
            PRESENT = false;
            return false;
        }
        if !wait_drq() {
            PRESENT = false;
            return false;
        }

        let mut id = [0u16; 256];
        for w in id.iter_mut() {
            *w = inw(ATA_PRIMARY_IO + ATA_REG_DATA);
        }
        // words 60-61: total user-addressable sectors (LBA28)
        TOTAL_SECTORS = id[60] as u32 | ((id[61] as u32) << 16);
        if TOTAL_SECTORS == 0 {
            TOTAL_SECTORS = 1024 * 1024; // assume large enough
        }
        PRESENT = true;
        true
    }
}

pub fn is_present() -> bool {
    unsafe { PRESENT }
}

pub fn sector_count() -> u32 {
    unsafe { TOTAL_SECTORS }
}

pub fn sector_size() -> usize {
    SECTOR_SIZE
}

/// Read one 512-byte sector at LBA into `buf` (must be >= 512).
pub fn read_sector(lba: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    if buf.len() < SECTOR_SIZE {
        return Err("buffer too small");
    }
    if !is_present() {
        return Err("no IDE disk");
    }
    unsafe {
        wait_bsy();
        outb(
            ATA_PRIMARY_IO + ATA_REG_DRIVE,
            0xE0 | ((lba >> 24) & 0x0F) as u8,
        );
        outb(ATA_PRIMARY_IO + ATA_REG_SECCOUNT, 1);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA0, lba as u8);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA1, (lba >> 8) as u8);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA2, (lba >> 16) as u8);
        outb(ATA_PRIMARY_IO + ATA_REG_CMD, ATA_CMD_READ_PIO);
        if !wait_drq() {
            return Err("IDE read timeout/error");
        }
        for i in 0..(SECTOR_SIZE / 2) {
            let w = inw(ATA_PRIMARY_IO + ATA_REG_DATA);
            buf[i * 2] = w as u8;
            buf[i * 2 + 1] = (w >> 8) as u8;
        }
        wait_bsy();
    }
    Ok(())
}

/// Write one 512-byte sector at LBA from `buf`.
pub fn write_sector(lba: u32, buf: &[u8]) -> Result<(), &'static str> {
    if buf.len() < SECTOR_SIZE {
        return Err("buffer too small");
    }
    if !is_present() {
        return Err("no IDE disk");
    }
    unsafe {
        wait_bsy();
        outb(
            ATA_PRIMARY_IO + ATA_REG_DRIVE,
            0xE0 | ((lba >> 24) & 0x0F) as u8,
        );
        outb(ATA_PRIMARY_IO + ATA_REG_SECCOUNT, 1);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA0, lba as u8);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA1, (lba >> 8) as u8);
        outb(ATA_PRIMARY_IO + ATA_REG_LBA2, (lba >> 16) as u8);
        outb(ATA_PRIMARY_IO + ATA_REG_CMD, ATA_CMD_WRITE_PIO);
        if !wait_drq() {
            return Err("IDE write timeout/error");
        }
        for i in 0..(SECTOR_SIZE / 2) {
            let lo = buf[i * 2] as u16;
            let hi = buf[i * 2 + 1] as u16;
            outw(ATA_PRIMARY_IO + ATA_REG_DATA, lo | (hi << 8));
        }
        outb(ATA_PRIMARY_IO + ATA_REG_CMD, ATA_CMD_CACHE_FLUSH);
        wait_bsy();
    }
    Ok(())
}

/// Read `count` sectors starting at `lba` into `buf`.
pub fn read_sectors(lba: u32, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    let need = (count as usize) * SECTOR_SIZE;
    if buf.len() < need {
        return Err("buffer too small");
    }
    for i in 0..count {
        let off = (i as usize) * SECTOR_SIZE;
        read_sector(lba + i, &mut buf[off..off + SECTOR_SIZE])?;
    }
    Ok(())
}

/// Write `count` sectors.
pub fn write_sectors(lba: u32, count: u32, buf: &[u8]) -> Result<(), &'static str> {
    let need = (count as usize) * SECTOR_SIZE;
    if buf.len() < need {
        return Err("buffer too small");
    }
    for i in 0..count {
        let off = (i as usize) * SECTOR_SIZE;
        write_sector(lba + i, &buf[off..off + SECTOR_SIZE])?;
    }
    Ok(())
}
