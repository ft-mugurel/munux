//! Functions to work on the **memory of a process** (stack/heap regions).

use super::pcb::Pid;
use super::table;
use crate::memory::paging::{self, PAGE_KERNEL_RW, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::memory::pmm::{self, FRAME_SIZE};

const PAGE_USER_RW: u64 = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

/// Cap user heap growth (absolute VA must stay below this and below the stack).
const USER_HEAP_MAX_VA: u64 = 0x0000_0000_7000_0000;
/// Max bytes a single process may grow via brk (16 MiB).
const USER_HEAP_MAX_BYTES: u64 = 16 * 1024 * 1024;

#[inline]
fn page_ceil(a: u64) -> u64 {
    (a + FRAME_SIZE as u64 - 1) & !(FRAME_SIZE as u64 - 1)
}

/// Reset the current process heap to `brk_start` (size 0). Used after exec / image load.
pub fn set_brk_start(brk_start: u64) {
    let _ = table::with_current(|p| {
        p.heap_base = brk_start;
        p.heap_size = 0;
    });
}

/// Current program break for the current process (`heap_base + heap_size`).
pub fn current_brk() -> u64 {
    table::with_current(|p| p.heap_base.saturating_add(p.heap_size)).unwrap_or(0)
}

/// Linux `brk(2)` for the **current** process.
///
/// On success returns the new program break. On failure (or when `new_brk` is
/// below the start break, including 0) returns the **unchanged** current break.
/// This matches the Linux syscall (not the libc wrapper that returns 0/-1).
pub fn proc_brk(new_brk: u64) -> u64 {
    table::with_current(|p| {
        // Reject kernel / unset bases (kinit inherits a high kernel heap VA).
        if p.heap_base < 0x1000 || p.heap_base >= 0x0000_8000_0000_0000 {
            return 0;
        }
        let start = p.heap_base;
        let old_brk = start.saturating_add(p.heap_size);

        // Query / invalid: leave break unchanged (Linux returns current brk).
        if new_brk < start {
            return old_brk;
        }

        let max_by_size = start.saturating_add(USER_HEAP_MAX_BYTES);
        let max_brk = max_by_size.min(USER_HEAP_MAX_VA);
        if new_brk > max_brk {
            return old_brk;
        }

        let old_pg = page_ceil(old_brk);
        let new_pg = page_ceil(new_brk);

        if new_pg > old_pg {
            let mut v = old_pg;
            while v < new_pg {
                // Prefer existing identity mapping; else allocate a fresh frame.
                if let Some(phys) = paging::virt_to_phys(v) {
                    let page = phys & !0xFFF;
                    paging::map_page(v, pmm::PhysAddr::new(page), PAGE_USER_RW);
                } else {
                    match pmm::alloc_frame() {
                        Some(frame) => paging::map_page(v, frame, PAGE_USER_RW),
                        None => return old_brk, // OOM: no change
                    }
                }
                unsafe {
                    core::ptr::write_bytes(v as *mut u8, 0, FRAME_SIZE);
                }
                v = v.wrapping_add(FRAME_SIZE as u64);
            }
        } else if new_pg < old_pg {
            let mut v = new_pg;
            while v < old_pg {
                paging::unmap_page(v);
                v = v.wrapping_add(FRAME_SIZE as u64);
            }
        }

        p.heap_size = new_brk.saturating_sub(start);
        new_brk
    })
    .unwrap_or(0)
}

/// Read `len` bytes from process virtual `addr` into `buf`.
pub fn proc_read_mem(pid: Pid, addr: u64, buf: &mut [u8]) -> Result<usize, i32> {
    let ok = table::with_pid(pid, |p| region_ok(p, addr, buf.len() as u64)).unwrap_or(false);
    if !ok {
        return Err(-1);
    }
    for (i, b) in buf.iter_mut().enumerate() {
        *b = unsafe { core::ptr::read_volatile((addr as usize + i) as *const u8) };
    }
    Ok(buf.len())
}

/// Write `buf` into process virtual memory at `addr`.
pub fn proc_write_mem(pid: Pid, addr: u64, buf: &[u8]) -> Result<usize, i32> {
    let ok = table::with_pid(pid, |p| region_ok(p, addr, buf.len() as u64)).unwrap_or(false);
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

fn region_ok(p: &super::pcb::Process, addr: u64, len: u64) -> bool {
    let end = addr.saturating_add(len);
    if p.stack_size > 0 {
        let s0 = p.stack_base;
        let s1 = p.stack_base.saturating_add(p.stack_size);
        if addr >= s0 && end <= s1 {
            return true;
        }
    }
    if p.heap_size > 0 || p.heap_base >= 0x1000 {
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
    // User tasks share kernel mapping today (no private page tables yet)
    true
}

/// Grow/shrink heap by `increment` (sbrk-style). Returns new break as i64, or -1.
pub fn proc_sbrk(pid: Pid, increment: i64) -> i64 {
    // Only meaningful for current process in our cooperative model.
    if table::with_pid(pid, |_| ()).is_none() {
        return -1;
    }
    if table::current_pid() != pid {
        return -1;
    }
    let old = current_brk();
    if increment == 0 {
        return old as i64;
    }
    let new = if increment > 0 {
        old.saturating_add(increment as u64)
    } else {
        let dec = (-increment) as u64;
        if dec > old {
            return -1;
        }
        old - dec
    };
    let got = proc_brk(new);
    if increment > 0 && got < new {
        -1
    } else if increment < 0 && got > new {
        -1
    } else {
        got as i64
    }
}

/// Allocate a private kernel stack for a process (returns base, size).
pub fn alloc_process_stack(size: usize) -> Option<(u64, u64)> {
    let size = (size + FRAME_SIZE - 1) & !(FRAME_SIZE - 1);
    // High kernel VA window for process stacks
    static mut NEXT_STACK_VA: u64 = 0x0000_0000_D000_0000;
    let base = unsafe {
        let b = NEXT_STACK_VA;
        NEXT_STACK_VA = NEXT_STACK_VA.wrapping_add(size as u64 + FRAME_SIZE as u64);
        b
    };
    let pages = size / FRAME_SIZE;
    let mut v = base;
    for _ in 0..pages {
        let frame = pmm::alloc_frame()?;
        paging::map_page(v, frame, PAGE_KERNEL_RW);
        v = v.wrapping_add(FRAME_SIZE as u64);
    }
    Some((base, size as u64))
}
