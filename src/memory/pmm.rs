//! Physical Memory Manager — 4 KiB frame bitmap (x86_64).

use core::ptr::{addr_of, addr_of_mut};

use crate::memory::multiboot;

/// Size of one physical frame / page.
pub const FRAME_SIZE: usize = 4096;

/// Track up to 4 GiB of physical RAM (enough for early munux / QEMU defaults).
const MAX_PHYS: u64 = 4 * 1024 * 1024 * 1024;
const MAX_FRAMES: usize = (MAX_PHYS / FRAME_SIZE as u64) as usize; // 1_048_576
const BITMAP_BYTES: usize = MAX_FRAMES / 8;

/// Kernel load address (matches linker / Multiboot load).
pub const KERNEL_LOAD_BASE: u64 = 0x0010_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn is_aligned(self) -> bool {
        self.0 % FRAME_SIZE as u64 == 0
    }

    pub fn frame_index(self) -> usize {
        (self.0 / FRAME_SIZE as u64) as usize
    }
}

// 1 = used, 0 = free
static mut BITMAP: [u8; BITMAP_BYTES] = [0xFF; BITMAP_BYTES];
static mut TOTAL_FRAMES: usize = 0;
static mut USED_FRAMES: usize = 0;
static mut HIGHEST_FRAME: usize = 0; // exclusive
static mut INITIALIZED: bool = false;

extern "C" {
    static kernel_end: u8;
}

#[inline]
unsafe fn bitmap_byte(byte: usize) -> *mut u8 {
    addr_of_mut!(BITMAP).cast::<u8>().add(byte)
}

fn set_used(frame: usize) {
    if frame >= MAX_FRAMES {
        return;
    }
    let byte = frame / 8;
    let bit = frame % 8;
    unsafe {
        let p = bitmap_byte(byte);
        let cur = p.read();
        let was_free = cur & (1 << bit) == 0;
        p.write(cur | (1 << bit));
        if was_free && frame < HIGHEST_FRAME {
            USED_FRAMES = USED_FRAMES.saturating_add(1);
        }
    }
}

fn set_free(frame: usize) {
    if frame >= MAX_FRAMES {
        return;
    }
    let byte = frame / 8;
    let bit = frame % 8;
    unsafe {
        let p = bitmap_byte(byte);
        let cur = p.read();
        let was_used = cur & (1 << bit) != 0;
        p.write(cur & !(1 << bit));
        if was_used && frame < HIGHEST_FRAME {
            USED_FRAMES = USED_FRAMES.saturating_sub(1);
        }
    }
}

fn is_used(frame: usize) -> bool {
    if frame >= MAX_FRAMES {
        return true;
    }
    let byte = frame / 8;
    let bit = frame % 8;
    unsafe { bitmap_byte(byte).read() & (1 << bit) != 0 }
}

fn mark_range_free(start: u64, end: u64) {
    let start = start.min(MAX_PHYS);
    let end = end.min(MAX_PHYS);
    if start >= end {
        return;
    }
    let first = ((start + FRAME_SIZE as u64 - 1) / FRAME_SIZE as u64) as usize;
    let last = (end / FRAME_SIZE as u64) as usize;
    for f in first.min(MAX_FRAMES)..last.min(MAX_FRAMES) {
        set_free(f);
    }
}

fn mark_range_used(start: u64, end: u64) {
    if end <= start {
        return;
    }
    let start = start.min(MAX_PHYS);
    let end = end.min(MAX_PHYS);
    if start >= end {
        return;
    }
    let first = (start / FRAME_SIZE as u64) as usize;
    let last = ((end + FRAME_SIZE as u64 - 1) / FRAME_SIZE as u64) as usize;
    for f in first.min(MAX_FRAMES)..last.min(MAX_FRAMES) {
        set_used(f);
    }
}

fn recount() {
    unsafe {
        let mut used = 0;
        for f in 0..HIGHEST_FRAME {
            if is_used(f) {
                used += 1;
            }
        }
        USED_FRAMES = used;
        TOTAL_FRAMES = HIGHEST_FRAME;
    }
}

fn reset_all_used() {
    unsafe {
        core::ptr::write_bytes(addr_of_mut!(BITMAP).cast::<u8>(), 0xFF, BITMAP_BYTES);
        USED_FRAMES = 0;
        TOTAL_FRAMES = 0;
        HIGHEST_FRAME = 0;
    }
}

/// Initialize PMM from Multiboot2 info (or 128 MiB fallback).
pub fn init(magic: u32, info_addr: u32) {
    unsafe {
        if INITIALIZED {
            return;
        }
    }
    reset_all_used();

    let mut max_end = 0u64;
    let mut regions = 0u32;

    let parsed = unsafe {
        multiboot::for_each_available_region(magic, info_addr, |base, end| {
            regions += 1;
            if end > max_end {
                max_end = end;
            }
            mark_range_free(base, end);
        })
    };

    if !parsed || max_end < 0x200000 {
        // Fallback: assume 128 MiB after 1 MiB hole
        max_end = 128 * 1024 * 1024;
        mark_range_free(0x0, 0xA0000);
        mark_range_free(0x100000, max_end);
        regions = 0;
    }

    let highest = ((max_end.min(MAX_PHYS)) / FRAME_SIZE as u64) as usize;
    unsafe {
        HIGHEST_FRAME = highest.min(MAX_FRAMES).max(1);
    }

    // Reserve firmware / low mem / VGA / GDT-era low areas
    mark_range_used(0, 0x100000);

    // Reserve kernel image (load base .. kernel_end)
    let kend = addr_of!(kernel_end) as u64;
    let kend = (kend + FRAME_SIZE as u64 - 1) & !(FRAME_SIZE as u64 - 1);
    mark_range_used(KERNEL_LOAD_BASE, kend);

    // Multiboot info structure (a few pages is enough)
    if info_addr != 0 {
        mark_range_used(info_addr as u64, info_addr as u64 + 4096);
    }

    // Bitmap storage itself lives in kernel BSS (inside kernel_end) — covered.

    recount();
    unsafe {
        INITIALIZED = true;
    }

    let _ = regions;
}

pub fn is_initialized() -> bool {
    unsafe { INITIALIZED }
}

pub fn total_frames() -> usize {
    unsafe { TOTAL_FRAMES }
}

pub fn used_frames() -> usize {
    unsafe { USED_FRAMES }
}

pub fn free_frames() -> usize {
    total_frames().saturating_sub(used_frames())
}

/// Allocate one 4 KiB frame; returns physical address (zeroed).
pub fn alloc_frame() -> Option<PhysAddr> {
    if !is_initialized() {
        return None;
    }
    unsafe {
        for frame in 0..HIGHEST_FRAME {
            if !is_used(frame) {
                set_used(frame);
                let addr = (frame as u64) * FRAME_SIZE as u64;
                // Identity-mapped for early kernel
                core::ptr::write_bytes(addr as *mut u8, 0, FRAME_SIZE);
                return Some(PhysAddr::new(addr));
            }
        }
    }
    None
}

pub fn free_frame(addr: PhysAddr) {
    if !is_initialized() || !addr.is_aligned() {
        return;
    }
    let frame = addr.frame_index();
    unsafe {
        if frame >= HIGHEST_FRAME {
            return;
        }
        if !is_used(frame) {
            return; // ignore double free in bring-up
        }
        set_free(frame);
    }
}
