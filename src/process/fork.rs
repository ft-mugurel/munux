//! fork — create a child PCB with copied metadata and saved user context (U6).

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

/// Private user stacks for fork children (shared page tables ⇒ need distinct VAs).
const CHILD_STACK_REGION: u64 = 0x0000_0000_6F00_0000;
const CHILD_STACK_STRIDE: u64 = 0x0000_0000_0002_0000; // 128 KiB per slot
const CHILD_STACK_PAGES: u64 = 4;

/// Fork current process. Parent stays current and returns child PID (>0).
/// Child is left **Ready** with `user_rax = 0` and a **private stack copy**.
///
/// Shares code/data pages (no page-table clone). Child is typically run to
/// completion inside `sys_fork` before the parent resumes (cooperative).
pub fn fork_from_user(user_rip: u64, user_rsp: u64, user_rflags: u64) -> Result<i32, i32> {
    let parent_idx = table::current_index();
    let parent_pid = table::current_pid();

    let (uid, heap_base, heap_size, cwd) =
        match table::with_current(|p| (p.uid, p.heap_base, p.heap_size, p.cwd_inode)) {
            Some(x) => x,
            None => return Err(-1),
        };

    let child_idx = match table::alloc_slot() {
        Some(i) => i,
        None => return Err(-1),
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

    // Copy user stack into a private VA range so parent/child do not clobber
    // each other under a shared address space.
    let (child_rsp, stack_base, stack_size) =
        match clone_user_stack(user_rsp, child_idx) {
            Some(x) => x,
            None => {
                table::free_index(child_idx);
                return Err(-1);
            }
        };

    let _ = table::with_pid(child_pid, |p| {
        p.cwd_inode = cwd;
        p.state = ProcessState::Ready;
        p.user_rip = user_rip;
        p.user_rsp = child_rsp;
        p.user_rflags = user_rflags | 0x200; // IF
        p.user_rax = 0; // child sees fork return 0
        p.stack_base = stack_base;
        p.stack_size = stack_size;
        p.set_name("forked");
    });

    if !table::add_child(parent_idx, child_pid) {
        table::free_index(child_idx);
        return Err(-1);
    }

    Ok(child_pid)
}

/// Map `CHILD_STACK_PAGES` at a per-slot VA and copy from the parent's stack window.
fn clone_user_stack(parent_rsp: u64, slot: usize) -> Option<(u64, u64, u64)> {
    let flags = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    let stack_size = CHILD_STACK_PAGES * FRAME_SIZE as u64;
    let child_base = CHILD_STACK_REGION + (slot as u64) * CHILD_STACK_STRIDE;

    // Infer parent stack window: prefer classic ELF stack, else page-align rsp.
    let elf_top = crate::elf::USER_STACK_TOP;
    let elf_base = elf_top - stack_size;
    let (parent_base, offset_in_stack) = if parent_rsp >= elf_base && parent_rsp <= elf_top {
        (elf_base, parent_rsp - elf_base)
    } else {
        // Demo stack or unknown: copy one page containing rsp
        let page = parent_rsp & !(FRAME_SIZE as u64 - 1);
        (page, parent_rsp - page)
    };

    let pages = if parent_rsp >= elf_base && parent_rsp <= elf_top {
        CHILD_STACK_PAGES
    } else {
        1
    };
    let copy_size = pages * FRAME_SIZE as u64;

    for i in 0..pages {
        let cv = child_base + i * FRAME_SIZE as u64;
        let frame = pmm::alloc_frame()?;
        paging::map_page(cv, frame, flags);
        let pv = parent_base + i * FRAME_SIZE as u64;
        unsafe {
            if paging::virt_to_phys(pv).is_some() {
                core::ptr::copy_nonoverlapping(
                    pv as *const u8,
                    cv as *mut u8,
                    FRAME_SIZE,
                );
            } else {
                core::ptr::write_bytes(cv as *mut u8, 0, FRAME_SIZE);
            }
        }
    }

    let child_rsp = child_base + offset_in_stack.min(copy_size.saturating_sub(8));
    Some((child_rsp, child_base, copy_size))
}

/// Legacy fork helper (kernel). Prefer [`fork_from_user`].
pub fn fork() -> i32 {
    fork_from_user(0, 0, 0x202).unwrap_or(-1)
}

/// Cooperative switch: make `pid` current.
pub fn switch_to(pid: i32) -> i32 {
    let cur = table::current_pid();
    if cur == pid {
        return 0;
    }
    let Some(idx) = table::find_pid(pid) else {
        return -1;
    };
    let _ = table::with_pid(cur, |p| {
        if p.state == ProcessState::Running {
            p.state = ProcessState::Ready;
        }
    });
    table::set_current_index(idx);
    let _ = table::with_pid(pid, |p| {
        p.state = ProcessState::Running;
    });
    0
}

/// Find a Ready child of the current process, switch current → child, return its frame.
pub fn take_ready_child(wait_for: i32) -> Option<UserFrame> {
    let parent = table::current_pid();
    let mut found_pid = -1i32;
    let mut frame = UserFrame {
        rip: 0,
        rsp: 0,
        rflags: 0x202,
        rax: 0,
    };

    table::for_each_process(|_idx, p| {
        if found_pid != -1 {
            return;
        }
        if p.used
            && p.state == ProcessState::Ready
            && p.parent == parent
            && (wait_for == -1 || wait_for == 0 || p.pid == wait_for)
        {
            found_pid = p.pid;
            frame = UserFrame {
                rip: p.user_rip,
                rsp: p.user_rsp,
                rflags: p.user_rflags,
                rax: p.user_rax,
            };
        }
    });

    if found_pid < 0 {
        return None;
    }

    let parent_idx = table::current_index();
    let _ = table::with_pid(parent, |p| {
        if p.state == ProcessState::Running {
            p.state = ProcessState::Sleeping;
        }
    });
    let _ = parent_idx;

    if let Some(idx) = table::find_pid(found_pid) {
        table::set_current_index(idx);
        let _ = table::with_pid(found_pid, |p| {
            p.state = ProcessState::Running;
        });
        Some(frame)
    } else {
        None
    }
}
