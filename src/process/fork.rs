//! fork — create a child PCB with private page tables and saved user context.

use super::pcb::ProcessState;
use super::table;
use crate::memory::paging::{self, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::memory::pmm::{self, FRAME_SIZE};

/// User frame to re-enter ring 3 (after fork schedule or execve).
#[derive(Clone, Copy, Debug)]
pub struct UserFrame {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub rax: u64,
}

/// User stack window size to copy on fork (BusyBox needs ~1 MiB).
const FORK_STACK_PAGES: u64 = 256; // 1 MiB

/// Fork current process. Parent stays current and returns child PID (>0).
/// Child is left **Ready** with `user_rax = 0`.
///
/// Private CR3 via [`clone_mm`]. Stack is copied to **new frames at the same
/// VAs** as the parent. Child is left **Ready**; caller (`wait`) schedules it.
pub fn fork_from_user(user_rip: u64, user_rsp: u64, user_rflags: u64) -> Result<i32, i32> {
    let parent_idx = table::current_index();
    let parent_pid = table::current_pid();

    let (uid, heap_base, heap_size, cwd, fs_base, gs_base, mmaps, mmap_bump, parent_cr3) =
        match table::with_current(|p| {
            let cr3 = if p.cr3 != 0 {
                p.cr3
            } else {
                crate::memory::kernel_cr3()
            };
            (
                p.uid,
                p.heap_base,
                p.heap_size,
                p.cwd_inode,
                p.fs_base,
                p.gs_base,
                p.mmaps,
                p.mmap_bump,
                cr3,
            )
        }) {
            Some(x) => x,
            None => return Err(-1),
        };

    let child_cr3 = match crate::memory::clone_mm(parent_cr3) {
        Some(c) => c,
        None => return Err(-1),
    };

    let child_idx = match table::alloc_slot() {
        Some(i) => i,
        None => {
            crate::memory::free_mm(child_cr3);
            return Err(-1);
        }
    };

    let mut child_pid = 0;
    table::init_child_slot(
        child_idx,
        parent_pid,
        uid,
        0,
        0,
        heap_base,
        heap_size,
        false,
        &mut child_pid,
    );

    // Record CR3 immediately so free_index / free_mm can reclaim on failure.
    let _ = table::with_pid(child_pid, |p| {
        p.cr3 = child_cr3;
    });

    // Per-process FDs: child gets a copy of parent's open table.
    crate::fd::clone_table(parent_idx, child_idx);

    // Private stack frames at the **same** VAs as the parent (private CR3).
    let (child_rsp, stack_base, stack_size) =
        match clone_user_stack_same_va(user_rsp, parent_cr3, child_cr3) {
            Some(x) => x,
            None => {
                table::free_index(child_idx);
                return Err(-1);
            }
        };

    let _ = table::with_pid(child_pid, |p| {
        p.cwd_inode = cwd;
        p.fs_base = fs_base;
        p.gs_base = gs_base;
        p.cr3 = child_cr3;
        p.mm_shared = false;
        p.tgid = child_pid; // new process = own thread group
        p.state = ProcessState::Ready;
        p.user_rip = user_rip;
        p.user_rsp = child_rsp;
        p.user_rflags = user_rflags | 0x200; // IF
        p.user_rax = 0; // child sees fork return 0
        // Synthetic trap so IRQ preemption can resume the child before wait.
        p.trap = crate::process::TrapFrame::from_user_entry(
            p.user_rip,
            p.user_rsp,
            p.user_rflags,
            p.user_rax,
        );
        p.trap_valid = true;
        p.entered_via_nest = false; // Ready after fork; may be IRQ-resumed
        p.stack_base = stack_base;
        p.stack_size = stack_size;
        p.mmaps = mmaps;
        p.mmap_bump = mmap_bump;
        p.clear_child_tid = 0;
        p.set_name("forked");
    });

    if !table::add_child(parent_idx, child_pid) {
        table::free_index(child_idx);
        return Err(-1);
    }

    Ok(child_pid)
}

/// Copy the parent stack window into `child_cr3` at the **same virtual addresses**,
/// using freshly allocated frames. `child_rsp == parent_rsp`.
fn clone_user_stack_same_va(
    parent_rsp: u64,
    parent_cr3: u64,
    child_cr3: u64,
) -> Option<(u64, u64, u64)> {
    let flags = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    let stack_size = FORK_STACK_PAGES * FRAME_SIZE as u64;

    let elf_top = crate::elf::USER_STACK_TOP;
    let elf_base = elf_top.saturating_sub(stack_size);
    let (stack_base, stack_size, child_rsp) = if parent_rsp >= elf_base && parent_rsp <= elf_top {
        (elf_base, stack_size, parent_rsp)
    } else if parent_rsp >= 0x1000 {
        let parent_top = (parent_rsp + FRAME_SIZE as u64 - 1) & !(FRAME_SIZE as u64 - 1);
        let base = parent_top.saturating_sub(stack_size).max(0x1000);
        let size = parent_top.saturating_sub(base);
        (base, size, parent_rsp)
    } else {
        return None;
    };

    let pages = (stack_size / FRAME_SIZE as u64).max(1);
    for i in 0..pages {
        let va = stack_base + i * FRAME_SIZE as u64;
        let frame = pmm::alloc_frame()?;
        // Always install a private frame in the child (replace shared clone leaf).
        paging::map_page_in(child_cr3, va, frame, flags);
        let dst = frame.as_u64() as *mut u8;
        unsafe {
            if let Some(pp) = paging::virt_to_phys_in(parent_cr3, va) {
                let src = (pp & !0xFFF) as *const u8;
                core::ptr::copy_nonoverlapping(src, dst, FRAME_SIZE);
            } else {
                core::ptr::write_bytes(dst, 0, FRAME_SIZE);
            }
        }
    }

    let top_page = (child_rsp.saturating_sub(8)) & !0xFFF;
    if paging::virt_to_phys_in(child_cr3, top_page).is_none() {
        return None;
    }
    Some((child_rsp, stack_base, stack_size))
}
