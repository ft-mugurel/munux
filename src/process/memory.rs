//! Functions to work on the **memory of a process** (stack/heap/mmap regions).

use super::pcb::{Pid, MAX_MMAPS};
use super::table;
use crate::memory::paging::{self, PAGE_KERNEL_RW, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::memory::pmm::{self, FRAME_SIZE};

const PAGE_USER_RW: u64 = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
const PAGE_USER_R: u64 = PAGE_PRESENT | PAGE_USER;
// x86 has no separate user-exec bit in our simple flags; R+X ≈ present+user.
const PAGE_USER_RX: u64 = PAGE_PRESENT | PAGE_USER;
const PAGE_USER_RWX: u64 = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

/// Cap user heap growth (absolute VA must stay below this and below the stack).
const USER_HEAP_MAX_VA: u64 = 0x0000_0000_7000_0000;
/// Max bytes a single process may grow via brk (16 MiB).
const USER_HEAP_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Anonymous mmap arena (below fork child stacks at 0x6F00_0000).
const MMAP_ARENA_BASE: u64 = 0x0000_0000_5000_0000;
const MMAP_ARENA_END: u64 = 0x0000_0000_6000_0000;
/// Max size of a single mmap request.
const MMAP_MAX_BYTES: u64 = 16 * 1024 * 1024;

// Linux mmap flags / prot (uapi)
pub const PROT_READ: u64 = 0x1;
pub const PROT_WRITE: u64 = 0x2;
pub const PROT_EXEC: u64 = 0x4;
pub const MAP_SHARED: u64 = 0x01;
pub const MAP_PRIVATE: u64 = 0x02;
pub const MAP_FIXED: u64 = 0x10;
pub const MAP_ANONYMOUS: u64 = 0x20;

#[inline]
fn page_ceil(a: u64) -> u64 {
    (a + FRAME_SIZE as u64 - 1) & !(FRAME_SIZE as u64 - 1)
}

fn prot_to_flags(prot: u64) -> u64 {
    let r = prot & PROT_READ != 0;
    let w = prot & PROT_WRITE != 0;
    let x = prot & PROT_EXEC != 0;
    match (r || x, w, x) {
        (_, true, _) => {
            // Writable implies present+user+writable (+exec not distinct).
            if x {
                PAGE_USER_RWX
            } else {
                PAGE_USER_RW
            }
        }
        (true, false, true) => PAGE_USER_RX,
        (true, false, false) => PAGE_USER_R,
        // PROT_NONE or empty: still map present so faults stay rare; no write.
        _ => PAGE_USER_R,
    }
}

fn map_anon_pages(start: u64, len: u64, page_flags: u64) -> Result<(), ()> {
    let mut v = start;
    let end = start.saturating_add(len);
    while v < end {
        if let Some(phys) = paging::virt_to_phys(v) {
            let page = phys & !0xFFF;
            paging::map_page(v, pmm::PhysAddr::new(page), page_flags);
        } else {
            let frame = pmm::alloc_frame().ok_or(())?;
            paging::map_page(v, frame, page_flags);
        }
        unsafe {
            core::ptr::write_bytes(v as *mut u8, 0, FRAME_SIZE);
        }
        v = v.wrapping_add(FRAME_SIZE as u64);
    }
    Ok(())
}

fn unmap_pages(start: u64, len: u64) {
    let mut v = start;
    let end = start.saturating_add(len);
    while v < end {
        paging::unmap_page(v);
        v = v.wrapping_add(FRAME_SIZE as u64);
    }
}

fn ranges_overlap(a: u64, alen: u64, b: u64, blen: u64) -> bool {
    let a_end = a.saturating_add(alen);
    let b_end = b.saturating_add(blen);
    a < b_end && b < a_end
}

/// Reset the current process heap to `brk_start` (size 0). Used after exec / image load.
pub fn set_brk_start(brk_start: u64) {
    let _ = table::with_current(|p| {
        p.heap_base = brk_start;
        p.heap_size = 0;
    });
}

/// Drop all anonymous mmaps for the current process (unmap pages + clear slots).
/// Called on exec so the new image does not inherit old maps.
pub fn clear_mmaps() {
    let _ = table::with_current(|p| {
        for i in 0..MAX_MMAPS {
            if p.mmaps[i].used {
                let a = p.mmaps[i].addr;
                let l = p.mmaps[i].len;
                unmap_pages(a, l);
                p.mmaps[i].used = false;
                p.mmaps[i].addr = 0;
                p.mmaps[i].len = 0;
            }
        }
        p.mmap_bump = 0;
    });
}

/// Linux-style anonymous `mmap` for the current process.
///
/// Supports `MAP_PRIVATE|MAP_ANONYMOUS` (and optional `MAP_FIXED`).
/// Returns mapped VA on success, or `Err(errno)` as positive errno code.
pub fn proc_mmap(
    addr: u64,
    length: u64,
    prot: u64,
    flags: u64,
    fd: u64,
    offset: u64,
) -> Result<u64, i64> {
    if length == 0 {
        return Err(22); // EINVAL
    }
    if length > MMAP_MAX_BYTES {
        return Err(12); // ENOMEM
    }
    // Only anonymous private maps for now (musl large malloc path).
    if flags & MAP_ANONYMOUS == 0 {
        return Err(22); // EINVAL — file-backed not implemented
    }
    if flags & MAP_PRIVATE == 0 && flags & MAP_SHARED == 0 {
        return Err(22);
    }
    if flags & MAP_SHARED != 0 {
        // Shared anon would need more machinery; reject for now.
        return Err(22);
    }
    if offset != 0 {
        return Err(22);
    }
    // fd is ignored for MAP_ANONYMOUS on Linux (often -1).
    let _ = fd;

    let len = page_ceil(length);
    let page_flags = prot_to_flags(prot);

    table::with_current(|p| {
        // Free slot?
        let mut slot = None;
        for i in 0..MAX_MMAPS {
            if !p.mmaps[i].used {
                slot = Some(i);
                break;
            }
        }
        let slot = match slot {
            Some(i) => i,
            None => return Err(12), // ENOMEM
        };

        let want_fixed = flags & MAP_FIXED != 0;
        let base = if want_fixed {
            if addr == 0 || addr & (FRAME_SIZE as u64 - 1) != 0 {
                return Err(22);
            }
            if addr < MMAP_ARENA_BASE || addr.saturating_add(len) > MMAP_ARENA_END {
                return Err(12);
            }
            // Overlap existing maps?
            for i in 0..MAX_MMAPS {
                if p.mmaps[i].used
                    && ranges_overlap(addr, len, p.mmaps[i].addr, p.mmaps[i].len)
                {
                    return Err(12);
                }
            }
            addr
        } else {
            // Kernel picks: bump allocator with simple wrap skip.
            let mut bump = if p.mmap_bump == 0 {
                MMAP_ARENA_BASE
            } else {
                p.mmap_bump
            };
            if bump < MMAP_ARENA_BASE {
                bump = MMAP_ARENA_BASE;
            }
            // Align bump
            bump = page_ceil(bump);
            if bump.saturating_add(len) > MMAP_ARENA_END {
                // try from base once
                bump = MMAP_ARENA_BASE;
            }
            if bump.saturating_add(len) > MMAP_ARENA_END {
                return Err(12);
            }
            // Skip overlaps (linear scan)
            let mut guard = 0;
            'place: loop {
                guard += 1;
                if guard > MAX_MMAPS + 2 {
                    return Err(12);
                }
                let mut hit = false;
                for i in 0..MAX_MMAPS {
                    if p.mmaps[i].used
                        && ranges_overlap(bump, len, p.mmaps[i].addr, p.mmaps[i].len)
                    {
                        bump = page_ceil(p.mmaps[i].addr.saturating_add(p.mmaps[i].len));
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    break 'place;
                }
                if bump.saturating_add(len) > MMAP_ARENA_END {
                    return Err(12);
                }
            }
            bump
        };

        if map_anon_pages(base, len, page_flags).is_err() {
            return Err(12);
        }

        p.mmaps[slot].used = true;
        p.mmaps[slot].addr = base;
        p.mmaps[slot].len = len;
        if !want_fixed {
            let next = base.saturating_add(len);
            if next > p.mmap_bump {
                p.mmap_bump = next;
            }
        }
        Ok(base)
    })
    .unwrap_or(Err(12))
}

/// Linux `munmap` for the current process. Exact region match required (whole map).
pub fn proc_munmap(addr: u64, length: u64) -> Result<(), i64> {
    if length == 0 {
        return Err(22);
    }
    if addr & (FRAME_SIZE as u64 - 1) != 0 {
        return Err(22);
    }
    let len = page_ceil(length);

    table::with_current(|p| {
        for i in 0..MAX_MMAPS {
            if p.mmaps[i].used && p.mmaps[i].addr == addr && p.mmaps[i].len == len {
                unmap_pages(addr, len);
                p.mmaps[i].used = false;
                p.mmaps[i].addr = 0;
                p.mmaps[i].len = 0;
                return Ok(());
            }
        }
        // Also allow munmap of a prefix/suffix later; for now exact only.
        // Linux allows partial unmap — map partial as EINVAL for simplicity if no match.
        Err(22)
    })
    .unwrap_or(Err(22))
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
