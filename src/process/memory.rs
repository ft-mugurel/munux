//! Process address-space helpers: brk, anonymous mmap, mprotect, munmap.

use super::pcb::MAX_MMAPS;
use super::table;
use crate::memory::paging::{self, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
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

/// Anonymous mmap arena (user half, below classic stack).
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

fn prot_to_flags(prot: u64) -> Option<u64> {
    let r = prot & PROT_READ != 0;
    let w = prot & PROT_WRITE != 0;
    let x = prot & PROT_EXEC != 0;
    if !r && !w && !x {
        return None; // PROT_NONE — leave inaccessible
    }
    Some(match (r || x, w, x) {
        (_, true, _) => {
            if x {
                PAGE_USER_RWX
            } else {
                PAGE_USER_RW
            }
        }
        (true, false, true) => PAGE_USER_RX,
        _ => PAGE_USER_R,
    })
}

fn map_anon_pages(start: u64, len: u64, page_flags: u64) -> Result<(), ()> {
    let mut v = start;
    let end = start.saturating_add(len);
    while v < end {
        // Always allocate a fresh frame and install USER permissions.
        // Reusing identity-map leaves can leave supervisor-only PTEs after a
        // 2 MiB split (user write → #PF protection, error=0x7).
        let frame = pmm::alloc_frame().ok_or(())?;
        paging::map_page(v, frame, page_flags);
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

fn fill_pages_from_fd(fd: u64, file_off: u64, dest: u64, copy_len: u64) -> Result<(), i64> {
    if copy_len == 0 {
        return Ok(());
    }
    let mut off = file_off;
    let mut dst = dest;
    let mut left = copy_len as usize;
    let mut tmp = [0u8; 512];
    while left > 0 {
        let chunk = left.min(tmp.len());
        let n = match crate::fd::sys_read_at(fd, off, &mut tmp[..chunk]) {
            Ok(0) => break, // EOF — remaining pages stay zero
            Ok(n) => n,
            Err(crate::fd::FdError::BadFd) => return Err(9),
            Err(crate::fd::FdError::IsDir) => return Err(21),
            Err(_) => return Err(14), // EFAULT / I/O
        };
        unsafe {
            core::ptr::copy_nonoverlapping(tmp.as_ptr(), dst as *mut u8, n);
        }
        dst = dst.saturating_add(n as u64);
        off = off.saturating_add(n as u64);
        left -= n;
    }
    Ok(())
}

/// Linux-style `mmap` for the current process.
///
/// - `MAP_PRIVATE|MAP_ANONYMOUS` (optional `MAP_FIXED`)
/// - `MAP_PRIVATE` file-backed: page-aligned `offset`, snapshot copy into new pages
///
/// `MAP_SHARED` is still EINVAL (no writeback). Returns mapped VA or `Err(errno)`.
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
    if flags & MAP_PRIVATE == 0 && flags & MAP_SHARED == 0 {
        return Err(22);
    }
    let anon = flags & MAP_ANONYMOUS != 0;
    if anon {
        if offset != 0 {
            return Err(22);
        }
        if flags & MAP_SHARED != 0 {
            return Err(22);
        }
    } else {
        if flags & MAP_PRIVATE == 0 {
            return Err(22); // shared file maps: no writeback yet
        }
        if offset & (FRAME_SIZE as u64 - 1) != 0 {
            return Err(22);
        }
        crate::fd::mmap_source_ok(fd)?;
    }

    let len = page_ceil(length);
    let page_flags = prot_to_flags(prot); // None => PROT_NONE

    let base = table::with_current(|p| {
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
            // Musl malloc places MAP_FIXED guards near brk (not only mmap arena).
            let end = addr.saturating_add(len);
            if addr < 0x1000 || end > USER_HEAP_MAX_VA {
                return Err(12);
            }
            // Drop bookkeeping for any overlapping prior maps (Linux replaces).
            for i in 0..MAX_MMAPS {
                if p.mmaps[i].used
                    && ranges_overlap(addr, len, p.mmaps[i].addr, p.mmaps[i].len)
                {
                    p.mmaps[i].used = false;
                    p.mmaps[i].addr = 0;
                    p.mmaps[i].len = 0;
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

        match page_flags {
            Some(pf) => {
                if map_anon_pages(base, len, pf).is_err() {
                    return Err(12);
                }
            }
            None => {
                // PROT_NONE guard: keep the VA reserved in our table but do not
                // tear down pages that may still be part of the brk heap.
                // (True no-access guards can come later via mprotect.)
                // Ensure the range exists as non-present so user faults cleanly
                // only when it was never brk-backed; if already mapped, leave it.
                let mut v = base;
                let end = base.saturating_add(len);
                while v < end {
                    if paging::virt_to_phys(v).is_none() {
                        // Reserve nothing — leave unmapped.
                    }
                    // If already mapped (brk), leave permissions as-is.
                    v = v.wrapping_add(FRAME_SIZE as u64);
                }
            }
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
    .unwrap_or(Err(12))?;

    if !anon {
        if let Err(e) = fill_pages_from_fd(fd, offset, base, length) {
            let _ = proc_munmap(base, length);
            return Err(e);
        }
    }
    Ok(base)
}

/// Linux `mprotect(2)` — update PTE flags for [addr, addr+len).
pub fn proc_mprotect(addr: u64, length: u64, prot: u64) -> Result<(), i64> {
    if addr & (FRAME_SIZE as u64 - 1) != 0 {
        return Err(22);
    }
    if length == 0 {
        return Ok(());
    }
    let len = page_ceil(length);
    let end = addr.saturating_add(len);
    if addr < 0x1000 || end > USER_HEAP_MAX_VA {
        return Err(12);
    }
    match prot_to_flags(prot) {
        None => {
            // PROT_NONE
            unmap_pages(addr, len);
            Ok(())
        }
        Some(flags) => {
            let mut v = addr;
            while v < end {
                // Remap with new flags; allocate if missing.
                if let Some(phys) = paging::virt_to_phys(v) {
                    let page = phys & !0xFFF;
                    paging::map_page(v, pmm::PhysAddr::new(page), flags);
                } else {
                    let frame = pmm::alloc_frame().ok_or(12i64)?;
                    paging::map_page(v, frame, flags);
                    unsafe {
                        core::ptr::write_bytes(v as *mut u8, 0, FRAME_SIZE);
                    }
                }
                v = v.wrapping_add(FRAME_SIZE as u64);
            }
            Ok(())
        }
    }
}

/// Linux `munmap` for the current process.
///
/// Prefer exact tracked region; otherwise unmap the page range (Linux allows
/// munmap of any mapped pages, including MAP_FIXED guards near brk).
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
        // Best-effort: unmap requested pages even if not in our table.
        if addr >= 0x1000 && addr.saturating_add(len) <= USER_HEAP_MAX_VA {
            unmap_pages(addr, len);
            return Ok(());
        }
        Err(22)
    })
    .unwrap_or(Err(22))
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
                // Fresh frames with USER|RW (avoid identity PTE without U bit).
                match pmm::alloc_frame() {
                    Some(frame) => paging::map_page(v, frame, PAGE_USER_RW),
                    None => return old_brk, // OOM: no change
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

