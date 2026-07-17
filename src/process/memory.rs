//! Functions to work on the **memory of a process** (stack/heap regions).

use super::pcb::Pid;
use super::table;
use crate::memory::paging::{self, PAGE_KERNEL_RW};
use crate::memory::pmm::{self, FRAME_SIZE};

/// Read `len` bytes from process virtual `addr` into `buf`.
pub fn proc_read_mem(pid: Pid, addr: u32, buf: &mut [u8]) -> Result<usize, i32> {
    let ok = table::with_pid(pid, |p| region_ok(p, addr, buf.len() as u32)).unwrap_or(false);
    if !ok {
        return Err(-1);
    }
    for (i, b) in buf.iter_mut().enumerate() {
        *b = unsafe { core::ptr::read_volatile((addr as usize + i) as *const u8) };
    }
    Ok(buf.len())
}

/// Write `buf` into process virtual memory at `addr`.
pub fn proc_write_mem(pid: Pid, addr: u32, buf: &[u8]) -> Result<usize, i32> {
    let ok = table::with_pid(pid, |p| region_ok(p, addr, buf.len() as u32)).unwrap_or(false);
    if !ok {
        return Err(-1);
    }
    for (i, &b) in buf.iter().enumerate() {
        unsafe {
            core::ptr::write_volatile((addr as usize + i) as *mut u8, b);
        }
    }
    Ok(buf.len())
}

fn region_ok(p: &super::pcb::Process, addr: u32, len: u32) -> bool {
    let end = addr.saturating_add(len);
    // Allow stack range
    if p.stack_size > 0 {
        let s0 = p.stack_base;
        let s1 = p.stack_base.saturating_add(p.stack_size);
        if addr >= s0 && end <= s1 {
            return true;
        }
    }
    // Allow heap range
    if p.heap_size > 0 {
        let h0 = p.heap_base;
        let h1 = p.heap_base.saturating_add(p.heap_size);
        if addr >= h0 && end <= h1 {
            return true;
        }
    }
    // Init process: allow kernel heap / identity low mem for shell
    if p.pid == 1 {
        return true;
    }
    false
}

/// Grow process heap by `bytes` (page-aligned mapping). Returns new heap size or -1.
pub fn proc_sbrk(pid: Pid, increment: i32) -> i32 {
    table::with_pid(pid, |p| {
        if increment == 0 {
            return p.heap_size as i32;
        }
        if increment < 0 {
            return -1;
        }
        let add = increment as u32;
        let pages = ((add as usize) + FRAME_SIZE - 1) / FRAME_SIZE;
        let start = p.heap_base.saturating_add(p.heap_size);
        let mut v = start;
        for _ in 0..pages {
            if let Some(frame) = pmm::alloc_frame() {
                paging::map_page(v, frame, PAGE_KERNEL_RW);
            } else {
                return -1;
            }
            v = v.wrapping_add(FRAME_SIZE as u32);
        }
        p.heap_size = p.heap_size.saturating_add((pages * FRAME_SIZE) as u32);
        p.heap_size as i32
    })
    .unwrap_or(-1)
}

/// Allocate a private kernel stack for a process (returns base).
pub fn alloc_process_stack(size: usize) -> Option<(u32, u32)> {
    let size = (size + FRAME_SIZE - 1) & !(FRAME_SIZE - 1);
    // Use high kernel VA window for process stacks: 0xD0000000+
    static mut NEXT_STACK_VA: u32 = 0xD000_0000;
    let base = unsafe {
        let b = NEXT_STACK_VA;
        NEXT_STACK_VA = NEXT_STACK_VA.wrapping_add(size as u32 + FRAME_SIZE as u32); // guard gap
        b
    };
    let pages = size / FRAME_SIZE;
    let mut v = base;
    for _ in 0..pages {
        let frame = pmm::alloc_frame()?;
        paging::map_page(v, frame, PAGE_KERNEL_RW);
        v = v.wrapping_add(FRAME_SIZE as u32);
    }
    Some((base, size as u32))
}
