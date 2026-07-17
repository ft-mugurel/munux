//! Physical Memory Manager (frame allocator).
//!
//! Tracks physical RAM in 4 KiB frames using a bitmap.
//! Does **not** enable paging; it only hands out physical addresses.

use core::ptr::{addr_of, addr_of_mut};

use crate::memory::multiboot::{self, MultibootInfo};
use crate::panic::kernel_panic;

/// Size of one physical frame (matches x86 page size).
pub const FRAME_SIZE: usize = 4096;

/// Maximum frames we track (4 GiB / 4 KiB = 1M frames → 128 KiB bitmap).
/// Note: on 32-bit, `usize` cannot hold 4 GiB as a byte count, so we count frames.
const MAX_FRAMES: usize = 1024 * 1024; // 1_048_576 frames = 4 GiB
const BITMAP_BYTES: usize = MAX_FRAMES / 8; // 131_072
/// Max physical byte address we manage (4 GiB). Do **not** cast this to
/// 32-bit `usize` when it equals 2^32 — that truncates to 0 on i686.
const MAX_PHYS_EXCLUSIVE: u64 = (MAX_FRAMES as u64) * (FRAME_SIZE as u64);

/// Physical address wrapper (identity-usable before paging).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub u32);

impl PhysAddr {
    pub const fn new(addr: u32) -> Self {
        Self(addr)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    pub fn is_aligned(self) -> bool {
        (self.0 as usize) % FRAME_SIZE == 0
    }

    pub fn frame_index(self) -> usize {
        (self.0 as usize) / FRAME_SIZE
    }
}

// Bitmap: 1 = used / unusable, 0 = free
// Access only via raw pointers (avoids `static_mut_refs` warnings / UB footguns).
static mut BITMAP: [u8; BITMAP_BYTES] = [0xFF; BITMAP_BYTES];
static mut TOTAL_FRAMES: usize = 0;
static mut USED_FRAMES: usize = 0;
static mut HIGHEST_FRAME: usize = 0; // exclusive
static mut INITIALIZED: bool = false;

const MAX_ALLOCS: usize = 256;

#[derive(Clone, Copy)]
struct PhysAllocation {
    in_use: bool,
    start: u32,
    bytes: usize,
}

static mut ALLOCS: [PhysAllocation; MAX_ALLOCS] = [PhysAllocation {
    in_use: false,
    start: 0,
    bytes: 0,
}; MAX_ALLOCS];

extern "C" {
    static kernel_end: u8;
    static stack_bottom: u8;
    static stack_top: u8;
}

/// Kernel load base (matches linker script `. = 1M`).
pub const KERNEL_LOAD_BASE: u32 = 0x0010_0000;

#[inline]
unsafe fn bitmap_byte_ptr(byte: usize) -> *mut u8 {
    addr_of_mut!(BITMAP).cast::<u8>().add(byte)
}

#[inline]
unsafe fn alloc_ptr(i: usize) -> *mut PhysAllocation {
    addr_of_mut!(ALLOCS).cast::<PhysAllocation>().add(i)
}

fn bitmap_set_used(frame: usize) {
    if frame >= MAX_FRAMES {
        return;
    }
    let byte = frame / 8;
    let bit = frame % 8;
    unsafe {
        let p = bitmap_byte_ptr(byte);
        let cur = p.read();
        let was_free = cur & (1 << bit) == 0;
        p.write(cur | (1 << bit));
        if was_free && frame < HIGHEST_FRAME {
            USED_FRAMES = USED_FRAMES.saturating_add(1);
        }
    }
}

fn bitmap_set_free(frame: usize) {
    if frame >= MAX_FRAMES {
        return;
    }
    let byte = frame / 8;
    let bit = frame % 8;
    unsafe {
        let p = bitmap_byte_ptr(byte);
        let cur = p.read();
        let was_used = cur & (1 << bit) != 0;
        p.write(cur & !(1 << bit));
        if was_used && frame < HIGHEST_FRAME {
            USED_FRAMES = USED_FRAMES.saturating_sub(1);
        }
    }
}

fn bitmap_is_used(frame: usize) -> bool {
    if frame >= MAX_FRAMES {
        return true;
    }
    let byte = frame / 8;
    let bit = frame % 8;
    unsafe { bitmap_byte_ptr(byte).read() & (1 << bit) != 0 }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

fn clamp_phys_end(end: u64) -> u64 {
    end.min(MAX_PHYS_EXCLUSIVE)
}

fn mark_region_free(start: u64, end: u64) {
    let start = start.min(MAX_PHYS_EXCLUSIVE);
    let end = clamp_phys_end(end);
    if start >= end {
        return;
    }
    // Align in u64 space, then convert frame indices (always < MAX_FRAMES).
    let first = ((start + FRAME_SIZE as u64 - 1) / FRAME_SIZE as u64) as usize;
    let last = (end / FRAME_SIZE as u64) as usize;
    let first = first.min(MAX_FRAMES);
    let last = last.min(MAX_FRAMES);
    for frame in first..last {
        bitmap_set_free(frame);
    }
}

fn mark_region_used(start: u64, end: u64) {
    if end <= start {
        return;
    }
    let start = start.min(MAX_PHYS_EXCLUSIVE);
    let end = clamp_phys_end(end);
    if start >= end {
        return;
    }
    let first = (start / FRAME_SIZE as u64) as usize;
    let last = ((end + FRAME_SIZE as u64 - 1) / FRAME_SIZE as u64) as usize;
    let first = first.min(MAX_FRAMES);
    let last = last.min(MAX_FRAMES);
    for frame in first..last {
        bitmap_set_used(frame);
    }
}

fn recount_used() {
    unsafe {
        let mut used = 0;
        for f in 0..HIGHEST_FRAME {
            if bitmap_is_used(f) {
                used += 1;
            }
        }
        USED_FRAMES = used;
        TOTAL_FRAMES = HIGHEST_FRAME;
    }
}

fn reset_bitmap_all_used() {
    unsafe {
        let base = addr_of_mut!(BITMAP).cast::<u8>();
        core::ptr::write_bytes(base, 0xFF, BITMAP_BYTES);

        USED_FRAMES = 0;
        TOTAL_FRAMES = 0;
        HIGHEST_FRAME = 0;

        for i in 0..MAX_ALLOCS {
            alloc_ptr(i).write(PhysAllocation {
                in_use: false,
                start: 0,
                bytes: 0,
            });
        }
    }
}

/// Initialize PMM from Multiboot information.
pub fn init(magic: u32, info_addr: u32) {
    unsafe {
        if INITIALIZED {
            return;
        }
    }
    reset_bitmap_all_used();

    crate::println!("PMM: multiboot magic={:#x} info={:#x}", magic, info_addr);

    match unsafe { multiboot::load(magic, info_addr) } {
        Some(info) => init_from_multiboot(info_addr, info),
        None => {
            crate::println!("PMM: Multiboot info missing/invalid — fallback 128 MiB");
            init_fallback_ram(info_addr, 128);
        }
    }

    // If discovery failed (too little RAM tracked / nothing free), force fallback.
    if total_frames() < 1024 || free_frames() < 256 {
        crate::println!(
            "PMM: discovery too small (total={} free={}) — fallback 128 MiB",
            total_frames(),
            free_frames()
        );
        reset_bitmap_all_used();
        init_fallback_ram(info_addr, 128);
    }

    unsafe {
        INITIALIZED = true;
    }

    crate::println!(
        "PMM: ready  frames total={} used={} free={} ({} KiB free)",
        total_frames(),
        used_frames(),
        free_frames(),
        free_frames() * FRAME_SIZE / 1024
    );
}

fn init_from_multiboot(info_addr: u32, info: &MultibootInfo) {
    let mut found_mmap = false;
    let mut max_end: u64 = 0;
    let mut avail = 0u32;

    for entry in info.mmap_entries() {
        found_mmap = true;
        let end = entry.end();
        if end > max_end {
            max_end = end;
        }
        if entry.is_available() {
            avail += 1;
            mark_region_free(entry.base(), end);
        }
    }

    // Also honor basic mem_upper when present (extends max_end / free region).
    if info.has_mem() {
        let mem_upper = info.mem_upper_kib();
        let end = 0x100000u64 + (mem_upper as u64) * 1024;
        if end > max_end {
            max_end = end;
        }
        mark_region_free(0x0, 0xA0000);
        mark_region_free(0x100000, end);
        crate::println!(
            "PMM: mem_upper={} KiB mmap_entries={} avail={}",
            mem_upper,
            if found_mmap { avail } else { 0 },
            avail
        );
    } else if found_mmap {
        crate::println!("PMM: mmap avail entries={}", avail);
    } else {
        crate::println!("PMM: no mmap/mem — fallback 128 MiB");
        init_fallback_ram(info_addr, 128);
        return;
    }

    if max_end < 0x200000 {
        // Less than 2 MiB discovered — unusable
        crate::println!("PMM: max_end={:#x} too small — fallback", max_end);
        init_fallback_ram(info_addr, 128);
        return;
    }

    // Compute frame count in u64 so a 4 GiB end does not truncate to 0 on i686.
    let highest_u64 = clamp_phys_end(max_end) / FRAME_SIZE as u64;
    let highest = (highest_u64 as usize).min(MAX_FRAMES).max(1);
    unsafe {
        HIGHEST_FRAME = highest;
    }
    reserve_critical(info_addr, Some(info));
    recount_used();
}

fn init_fallback_ram(info_addr: u32, mib: u32) {
    let end = (mib as u64) * 1024 * 1024;
    mark_region_free(0x0, 0xA0000);
    mark_region_free(0x100000, end);
    unsafe {
        HIGHEST_FRAME = (end as usize / FRAME_SIZE).min(MAX_FRAMES);
    }
    reserve_critical(info_addr, None);
    recount_used();
}

fn reserve_critical(info_addr: u32, info: Option<&MultibootInfo>) {
    // First 1 MiB: firmware, VGA, GDT@0x800, …
    mark_region_used(0, 0x100000);

    // Kernel image through BSS end
    let kstart = KERNEL_LOAD_BASE as u64;
    let kend = align_up(addr_of!(kernel_end) as usize, FRAME_SIZE) as u64;
    mark_region_used(kstart, kend);

    let sbot = addr_of!(stack_bottom) as u64;
    let stop = addr_of!(stack_top) as u64;
    mark_region_used(sbot, stop);

    if info_addr != 0 {
        mark_region_used(info_addr as u64, info_addr as u64 + 256);
        if let Some(info) = info {
            if info.has_mmap() {
                let mmap_addr = info.mmap_addr();
                let mmap_length = info.mmap_length();
                if mmap_addr != 0 && mmap_length != 0 {
                    mark_region_used(
                        mmap_addr as u64,
                        mmap_addr as u64 + mmap_length as u64,
                    );
                }
            }
        }
    }
}

pub fn frame_size() -> usize {
    FRAME_SIZE
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

pub fn is_initialized() -> bool {
    unsafe { INITIALIZED }
}

/// Allocate one physical frame (4 KiB).
pub fn alloc_frame() -> Option<PhysAddr> {
    if !is_initialized() {
        kernel_panic("PMM: alloc_frame before init");
    }
    unsafe {
        for frame in 0..HIGHEST_FRAME {
            if !bitmap_is_used(frame) {
                bitmap_set_used(frame);
                let addr = (frame * FRAME_SIZE) as u32;
                core::ptr::write_bytes(addr as *mut u8, 0, FRAME_SIZE);
                return Some(PhysAddr::new(addr));
            }
        }
    }
    None
}

/// Free one physical frame from [`alloc_frame`].
pub fn free_frame(addr: PhysAddr) {
    if !is_initialized() {
        kernel_panic("PMM: free_frame before init");
    }
    if !addr.is_aligned() {
        kernel_panic("PMM: free_frame address not frame-aligned");
    }
    let frame = addr.frame_index();
    unsafe {
        if frame >= HIGHEST_FRAME {
            kernel_panic("PMM: free_frame out of range");
        }
        if !bitmap_is_used(frame) {
            kernel_panic("PMM: double free_frame");
        }
        bitmap_set_free(frame);
    }
}

fn frames_for_size(size: usize) -> usize {
    if size == 0 {
        1
    } else {
        align_up(size, FRAME_SIZE) / FRAME_SIZE
    }
}

fn register_alloc(start: u32, bytes: usize) -> bool {
    unsafe {
        for i in 0..MAX_ALLOCS {
            let p = alloc_ptr(i);
            let mut a = p.read();
            if !a.in_use {
                a.in_use = true;
                a.start = start;
                a.bytes = bytes;
                p.write(a);
                return true;
            }
        }
    }
    false
}

fn take_alloc(start: u32) -> Option<usize> {
    unsafe {
        for i in 0..MAX_ALLOCS {
            let p = alloc_ptr(i);
            let mut a = p.read();
            if a.in_use && a.start == start {
                let bytes = a.bytes;
                a.in_use = false;
                a.start = 0;
                a.bytes = 0;
                p.write(a);
                return Some(bytes);
            }
        }
    }
    None
}

fn peek_alloc(start: u32) -> Option<usize> {
    unsafe {
        for i in 0..MAX_ALLOCS {
            let a = alloc_ptr(i).read();
            if a.in_use && a.start == start {
                return Some(a.bytes);
            }
        }
    }
    None
}

/// Allocate physical memory of at least `size` bytes (rounded up to frames).
pub fn phys_alloc(size: usize) -> Option<PhysAddr> {
    if !is_initialized() {
        kernel_panic("PMM: phys_alloc before init");
    }
    let n = frames_for_size(size);
    let bytes = n * FRAME_SIZE;

    unsafe {
        let mut start = 0usize;
        while start + n <= HIGHEST_FRAME {
            let mut ok = true;
            for f in start..start + n {
                if bitmap_is_used(f) {
                    ok = false;
                    start = f + 1;
                    break;
                }
            }
            if !ok {
                continue;
            }
            for f in start..start + n {
                bitmap_set_used(f);
            }
            let addr = (start * FRAME_SIZE) as u32;
            core::ptr::write_bytes(addr as *mut u8, 0, bytes);
            if !register_alloc(addr, bytes) {
                for f in start..start + n {
                    bitmap_set_free(f);
                }
                return None;
            }
            return Some(PhysAddr::new(addr));
        }
    }
    None
}

/// Free a block from [`phys_alloc`], or a single frame from [`alloc_frame`].
pub fn phys_free(addr: PhysAddr) {
    if let Some(bytes) = take_alloc(addr.as_u32()) {
        let n = bytes / FRAME_SIZE;
        let start = addr.frame_index();
        for f in start..start + n {
            if !bitmap_is_used(f) {
                kernel_panic("PMM: phys_free: frame already free");
            }
            bitmap_set_free(f);
        }
        return;
    }
    free_frame(addr);
}

/// Size in bytes of a physical allocation, if known.
pub fn phys_size(addr: PhysAddr) -> Option<usize> {
    if let Some(bytes) = peek_alloc(addr.as_u32()) {
        return Some(bytes);
    }
    if addr.is_aligned() {
        let f = addr.frame_index();
        unsafe {
            if f < HIGHEST_FRAME && bitmap_is_used(f) {
                return Some(FRAME_SIZE);
            }
        }
    }
    None
}
