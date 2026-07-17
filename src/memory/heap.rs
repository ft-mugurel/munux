//! Kernel heap — byte allocator over virtually mapped pages (step 4).
//!
//! Virtual range: [`KERNEL_HEAP_START`] .. + [`KERNEL_HEAP_MAX`] (kernel space).
//! Grows by mapping frames with [`paging::create_page`].
//!
//! APIs:
//! - [`kmalloc`] / [`kfree`] / [`ksize`]  — “variables”
//! - [`virt_alloc`] / [`virt_free`] / [`virt_size`] — same (virtual memory wording)

use core::mem::size_of;
use core::ptr;

use crate::memory::paging::{self, PAGE_KERNEL_RW};
use crate::memory::pmm::FRAME_SIZE;
use crate::panic::kernel_panic;

/// Kernel heap base (high half / kernel space).
pub const KERNEL_HEAP_START: u32 = 0xC000_0000;

/// Max heap size (4 MiB).
pub const KERNEL_HEAP_MAX: u32 = 4 * 1024 * 1024;

const MAGIC_USED: u32 = 0x4845_4150; // "HEAP"
const MAGIC_FREE: u32 = 0x4652_4545; // "FREE"
const ALIGN: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct BlockHeader {
    magic: u32,
    /// Total block size including this header.
    size: u32,
    /// Next free block address (only when free). 0 = end.
    next_free: u32,
}

const HEADER_SIZE: usize = size_of::<BlockHeader>();

static mut HEAP_END: u32 = KERNEL_HEAP_START;
static mut FREE_HEAD: u32 = 0;
static mut INITIALIZED: bool = false;
static mut BYTES_ALLOCATED: usize = 0;
static mut ALLOC_COUNT: usize = 0;

fn align_up(n: usize, a: usize) -> usize {
    (n + a - 1) & !(a - 1)
}

unsafe fn hdr_read(addr: u32) -> BlockHeader {
    ptr::read_volatile(addr as *const BlockHeader)
}

unsafe fn hdr_write(addr: u32, h: BlockHeader) {
    ptr::write_volatile(addr as *mut BlockHeader, h);
}

fn payload(addr: u32) -> *mut u8 {
    (addr as usize + HEADER_SIZE) as *mut u8
}

fn header_of(ptr: *mut u8) -> u32 {
    (ptr as u32).wrapping_sub(HEADER_SIZE as u32)
}

pub fn init() {
    unsafe {
        if INITIALIZED {
            return;
        }
        HEAP_END = KERNEL_HEAP_START;
        FREE_HEAD = 0;
        BYTES_ALLOCATED = 0;
        ALLOC_COUNT = 0;
        INITIALIZED = true;
    }
    crate::println!(
        "heap: ready  VA [{:#x} .. {:#x})  max {} KiB",
        KERNEL_HEAP_START,
        KERNEL_HEAP_START + KERNEL_HEAP_MAX,
        KERNEL_HEAP_MAX / 1024
    );
}

pub fn is_initialized() -> bool {
    unsafe { INITIALIZED }
}

pub fn heap_start() -> u32 {
    KERNEL_HEAP_START
}

pub fn heap_end() -> u32 {
    unsafe { HEAP_END }
}

pub fn heap_used_bytes() -> usize {
    unsafe { BYTES_ALLOCATED }
}

pub fn heap_alloc_count() -> usize {
    unsafe { ALLOC_COUNT }
}

/// Map at least `min_bytes` more virtual memory onto the heap.
fn grow_heap(min_bytes: usize) -> bool {
    let pages = align_up(min_bytes.max(1), FRAME_SIZE) / FRAME_SIZE;
    unsafe {
        let add = (pages * FRAME_SIZE) as u32;
        if HEAP_END.saturating_add(add) > KERNEL_HEAP_START + KERNEL_HEAP_MAX {
            return false;
        }
        let old = HEAP_END;
        let mut v = HEAP_END;
        for _ in 0..pages {
            paging::create_page(v, PAGE_KERNEL_RW);
            v = v.wrapping_add(FRAME_SIZE as u32);
        }
        HEAP_END = v;

        hdr_write(
            old,
            BlockHeader {
                magic: MAGIC_FREE,
                size: HEAP_END - old,
                next_free: 0,
            },
        );
        insert_free(old);
        true
    }
}

unsafe fn insert_free(addr: u32) {
    // Keep free list sorted by address for easy coalescing.
    let mut prev = 0u32;
    let mut cur = FREE_HEAD;
    while cur != 0 && cur < addr {
        prev = cur;
        cur = hdr_read(cur).next_free;
    }

    let mut h = hdr_read(addr);
    h.magic = MAGIC_FREE;
    h.next_free = cur;
    hdr_write(addr, h);

    if prev == 0 {
        FREE_HEAD = addr;
    } else {
        let mut ph = hdr_read(prev);
        ph.next_free = addr;
        hdr_write(prev, ph);
    }

    coalesce(addr);
}

unsafe fn coalesce(addr: u32) {
    let mut this = hdr_read(addr);

    // Merge with next if adjacent.
    let next = this.next_free;
    if next != 0 && addr + this.size == next {
        let nh = hdr_read(next);
        this.size += nh.size;
        this.next_free = nh.next_free;
        this.magic = MAGIC_FREE;
        hdr_write(addr, this);
    }

    // Find previous free block.
    let mut prev = 0u32;
    let mut cur = FREE_HEAD;
    while cur != 0 && cur < addr {
        prev = cur;
        cur = hdr_read(cur).next_free;
    }

    if prev != 0 {
        let ph = hdr_read(prev);
        if prev + ph.size == addr {
            let th = hdr_read(addr);
            let mut merged = ph;
            merged.size = ph.size + th.size;
            merged.next_free = th.next_free;
            merged.magic = MAGIC_FREE;
            hdr_write(prev, merged);
        }
    }
}

/// Allocate `size` payload bytes in kernel virtual memory.
pub fn kmalloc(size: usize) -> Option<*mut u8> {
    if !is_initialized() {
        kernel_panic("heap: kmalloc before init");
    }
    if size == 0 {
        return None;
    }

    let need = align_up(HEADER_SIZE + size, ALIGN) as u32;

    unsafe {
        for _ in 0..16 {
            if let Some(p) = alloc_from_free(need) {
                // Track requested size approximately via usable region.
                BYTES_ALLOCATED = BYTES_ALLOCATED.saturating_add(size);
                ALLOC_COUNT = ALLOC_COUNT.saturating_add(1);
                // Zero only the requested payload.
                ptr::write_bytes(p, 0, size);
                return Some(p);
            }
            if !grow_heap(need as usize) {
                return None;
            }
        }
    }
    None
}

unsafe fn alloc_from_free(need: u32) -> Option<*mut u8> {
    let mut prev = 0u32;
    let mut cur = FREE_HEAD;

    while cur != 0 {
        let h = hdr_read(cur);
        if h.magic != MAGIC_FREE {
            kernel_panic("heap: free-list corruption");
        }
        if h.size >= need {
            // Unlink
            if prev == 0 {
                FREE_HEAD = h.next_free;
            } else {
                let mut ph = hdr_read(prev);
                ph.next_free = h.next_free;
                hdr_write(prev, ph);
            }

            let remain = h.size - need;
            if remain as usize >= HEADER_SIZE + ALIGN {
                let split = cur + need;
                hdr_write(
                    split,
                    BlockHeader {
                        magic: MAGIC_FREE,
                        size: remain,
                        next_free: 0,
                    },
                );
                insert_free(split);
                hdr_write(
                    cur,
                    BlockHeader {
                        magic: MAGIC_USED,
                        size: need,
                        next_free: 0,
                    },
                );
            } else {
                hdr_write(
                    cur,
                    BlockHeader {
                        magic: MAGIC_USED,
                        size: h.size,
                        next_free: 0,
                    },
                );
            }
            return Some(payload(cur));
        }
        prev = cur;
        cur = h.next_free;
    }
    None
}

/// Free memory from [`kmalloc`].
pub fn kfree(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    if !is_initialized() {
        kernel_panic("heap: kfree before init");
    }

    let addr = header_of(ptr);
    unsafe {
        if addr < KERNEL_HEAP_START || addr >= HEAP_END {
            kernel_panic("heap: kfree outside heap");
        }
        let h = hdr_read(addr);
        if h.magic != MAGIC_USED {
            kernel_panic("heap: kfree bad magic (double free?)");
        }
        let usable = h.size as usize - HEADER_SIZE;
        BYTES_ALLOCATED = BYTES_ALLOCATED.saturating_sub(usable);
        ALLOC_COUNT = ALLOC_COUNT.saturating_sub(1);

        hdr_write(
            addr,
            BlockHeader {
                magic: MAGIC_FREE,
                size: h.size,
                next_free: 0,
            },
        );
        insert_free(addr);
    }
}

/// Usable size of a [`kmalloc`] block (payload capacity).
pub fn ksize(ptr: *mut u8) -> Option<usize> {
    if ptr.is_null() || !is_initialized() {
        return None;
    }
    let addr = header_of(ptr);
    unsafe {
        if addr < KERNEL_HEAP_START || addr >= HEAP_END {
            return None;
        }
        let h = hdr_read(addr);
        if h.magic != MAGIC_USED {
            return None;
        }
        Some(h.size as usize - HEADER_SIZE)
    }
}

// --- Subject-facing virtual memory names ----------------------------------------

pub fn virt_alloc(size: usize) -> Option<*mut u8> {
    kmalloc(size)
}

pub fn virt_free(ptr: *mut u8) {
    kfree(ptr);
}

pub fn virt_size(ptr: *mut u8) -> Option<usize> {
    ksize(ptr)
}
