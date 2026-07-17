//! Kernel heap — freelist over virtually mapped pages (x86_64).

use core::mem::size_of;
use core::ptr;

use crate::memory::paging::{self, PAGE_KERNEL_RW};
use crate::memory::pmm::FRAME_SIZE;

/// Heap virtual base (outside 1 GiB identity map; pages mapped on demand).
pub const KERNEL_HEAP_START: u64 = 0x0000_0001_0000_0000;

/// Max heap size (4 MiB).
pub const KERNEL_HEAP_MAX: u64 = 4 * 1024 * 1024;

const MAGIC_USED: u32 = 0x4845_4150; // HEAP
const MAGIC_FREE: u32 = 0x4652_4545; // FREE
const ALIGN: usize = 16;

#[repr(C)]
#[derive(Clone, Copy)]
struct BlockHeader {
    magic: u32,
    size: u32, // total including header
    next_free: u64,
}

const HEADER_SIZE: usize = size_of::<BlockHeader>();

static mut HEAP_END: u64 = KERNEL_HEAP_START;
static mut FREE_HEAD: u64 = 0;
static mut INITIALIZED: bool = false;
static mut BYTES_ALLOCATED: usize = 0;
static mut ALLOC_COUNT: usize = 0;

fn align_up(n: usize, a: usize) -> usize {
    (n + a - 1) & !(a - 1)
}

unsafe fn hdr_read(addr: u64) -> BlockHeader {
    ptr::read_volatile(addr as *const BlockHeader)
}

unsafe fn hdr_write(addr: u64, h: BlockHeader) {
    ptr::write_volatile(addr as *mut BlockHeader, h);
}

fn payload(addr: u64) -> *mut u8 {
    (addr as usize + HEADER_SIZE) as *mut u8
}

fn header_of(p: *mut u8) -> u64 {
    (p as u64).wrapping_sub(HEADER_SIZE as u64)
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
}

pub fn is_initialized() -> bool {
    unsafe { INITIALIZED }
}

pub fn heap_start() -> u64 {
    KERNEL_HEAP_START
}

pub fn heap_end() -> u64 {
    unsafe { HEAP_END }
}

pub fn heap_used_bytes() -> usize {
    unsafe { BYTES_ALLOCATED }
}

pub fn heap_alloc_count() -> usize {
    unsafe { ALLOC_COUNT }
}

fn grow_heap(min_bytes: usize) -> bool {
    let pages = align_up(min_bytes.max(1), FRAME_SIZE) / FRAME_SIZE;
    unsafe {
        let add = (pages * FRAME_SIZE) as u64;
        if HEAP_END.saturating_add(add) > KERNEL_HEAP_START + KERNEL_HEAP_MAX {
            return false;
        }
        let old = HEAP_END;
        let mut v = HEAP_END;
        for _ in 0..pages {
            paging::create_page(v, PAGE_KERNEL_RW);
            v = v.wrapping_add(FRAME_SIZE as u64);
        }
        HEAP_END = v;
        hdr_write(
            old,
            BlockHeader {
                magic: MAGIC_FREE,
                size: (HEAP_END - old) as u32,
                next_free: 0,
            },
        );
        insert_free(old);
        true
    }
}

unsafe fn insert_free(addr: u64) {
    let mut prev = 0u64;
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

unsafe fn coalesce(addr: u64) {
    let mut this = hdr_read(addr);
    let next = this.next_free;
    if next != 0 && addr + this.size as u64 == next {
        let nh = hdr_read(next);
        this.size += nh.size;
        this.next_free = nh.next_free;
        this.magic = MAGIC_FREE;
        hdr_write(addr, this);
    }
    let mut prev = 0u64;
    let mut cur = FREE_HEAD;
    while cur != 0 && cur < addr {
        prev = cur;
        cur = hdr_read(cur).next_free;
    }
    if prev != 0 {
        let ph = hdr_read(prev);
        if prev + ph.size as u64 == addr {
            let th = hdr_read(addr);
            let mut merged = ph;
            merged.size = ph.size + th.size;
            merged.next_free = th.next_free;
            merged.magic = MAGIC_FREE;
            hdr_write(prev, merged);
        }
    }
}

pub fn kmalloc(size: usize) -> Option<*mut u8> {
    if !is_initialized() || size == 0 {
        return None;
    }
    let need = align_up(HEADER_SIZE + size, ALIGN) as u32;
    unsafe {
        for _ in 0..16 {
            if let Some(p) = alloc_from_free(need) {
                BYTES_ALLOCATED = BYTES_ALLOCATED.saturating_add(size);
                ALLOC_COUNT = ALLOC_COUNT.saturating_add(1);
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
    let mut prev = 0u64;
    let mut cur = FREE_HEAD;
    while cur != 0 {
        let h = hdr_read(cur);
        if h.magic != MAGIC_FREE {
            return None; // corruption
        }
        if h.size >= need {
            if prev == 0 {
                FREE_HEAD = h.next_free;
            } else {
                let mut ph = hdr_read(prev);
                ph.next_free = h.next_free;
                hdr_write(prev, ph);
            }
            let remain = h.size - need;
            if remain as usize >= HEADER_SIZE + ALIGN {
                let split = cur + need as u64;
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

pub fn kfree(p: *mut u8) {
    if p.is_null() || !is_initialized() {
        return;
    }
    let addr = header_of(p);
    unsafe {
        if addr < KERNEL_HEAP_START || addr >= HEAP_END {
            return;
        }
        let h = hdr_read(addr);
        if h.magic != MAGIC_USED {
            return;
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

pub fn ksize(p: *mut u8) -> Option<usize> {
    if p.is_null() || !is_initialized() {
        return None;
    }
    let addr = header_of(p);
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
