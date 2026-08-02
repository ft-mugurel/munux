//! Global process table and current process.

use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicI32, Ordering};

use super::pcb::{Pid, Process, ProcessState, Uid, MAX_PROCESSES};

static mut TABLE: [Process; MAX_PROCESSES] = [Process::empty(); MAX_PROCESSES];
static mut CURRENT: usize = 0;
static NEXT_PID: AtomicI32 = AtomicI32::new(1);

pub fn init_table() {
    unsafe {
        for i in 0..MAX_PROCESSES {
            *slot_mut(i) = Process::empty();
        }
        let p = &mut *slot_mut(0);
        p.used = true;
        p.pid = 1;
        p.tgid = 1;
        p.parent = -1;
        p.state = ProcessState::Running;
        p.uid = 0;
        p.stack_base = 0;
        p.stack_size = 0;
        p.heap_base = crate::memory::KERNEL_HEAP_START;
        p.heap_size = 0;
        p.cwd_inode = 2; // ext2 root inode
        p.cr3 = crate::memory::kernel_cr3();
        p.kstack_top = super::kstack::top_for_slot(0);
        // Kernel-side init (pid 1). Userspace /bin/sh is a child (boot handoff).
        p.set_name("kinit");
        CURRENT = 0;
        NEXT_PID.store(2, Ordering::Relaxed);
        if p.cr3 != 0 {
            crate::memory::switch_mm(p.cr3);
        }
        super::kstack::install_for_slot(0);
    }
}

unsafe fn slot_mut(i: usize) -> *mut Process {
    addr_of_mut!(TABLE).cast::<Process>().add(i)
}

pub fn current_index() -> usize {
    unsafe { CURRENT }
}

pub fn set_current_index(i: usize) {
    unsafe {
        CURRENT = i;
    }
    let cr3 = unsafe {
        let p = &*slot_mut(i);
        if p.used && p.cr3 != 0 {
            p.cr3
        } else {
            crate::memory::kernel_cr3()
        }
    };
    crate::memory::switch_mm(cr3);
    // Per-task kernel stack for syscall / ring0 entry (TSS RSP0 + syscall_kstack).
    super::kstack::install_for_slot(i);
    // Restore TLS bases for the newly current process.
    crate::process::apply_tls();
}

pub fn current_pid() -> Pid {
    unsafe {
        let p = &*slot_mut(CURRENT);
        if p.used {
            p.pid
        } else {
            1
        }
    }
}

pub fn with_current<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Process) -> R,
{
    unsafe {
        let p = &mut *slot_mut(CURRENT);
        if p.used {
            Some(f(p))
        } else {
            None
        }
    }
}

pub fn with_pid<F, R>(pid: Pid, f: F) -> Option<R>
where
    F: FnOnce(&mut Process) -> R,
{
    let i = find_pid(pid)?;
    unsafe {
        let p = &mut *slot_mut(i);
        Some(f(p))
    }
}

pub fn find_pid(pid: Pid) -> Option<usize> {
    unsafe {
        for i in 0..MAX_PROCESSES {
            let p = &*slot_mut(i);
            if p.used && p.pid == pid {
                return Some(i);
            }
        }
    }
    None
}

pub fn alloc_slot() -> Option<usize> {
    unsafe {
        for i in 0..MAX_PROCESSES {
            let p = &mut *slot_mut(i);
            if !p.used {
                let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
                *p = Process::empty();
                p.used = true;
                p.pid = pid;
                return Some(i);
            }
        }
    }
    None
}

pub fn free_index(i: usize) {
    if i >= MAX_PROCESSES {
        return;
    }
    crate::fd::clear_table(i);
    // Drop private page tables (Phase 1b); never free the boot kernel CR3.
    // Shared mm (CLONE_VM): free only when no other live task still uses CR3.
    let (cr3, pid, parent, mm_shared) = unsafe {
        let p = &*slot_mut(i);
        if p.used {
            (p.cr3, p.pid, p.parent, p.mm_shared)
        } else {
            (0, 0, -1, false)
        }
    };
    // Unlink from parent children[] so nchildren does not leak (unit tests
    // free slots without wait/reap).
    if parent >= 0 {
        if let Some(pidx) = find_pid(parent) {
            remove_child(pidx, pid);
        }
    }
    let k = crate::memory::kernel_cr3();
    if cr3 != 0 && cr3 != k {
        let mut others = false;
        if mm_shared {
            unsafe {
                for j in 0..MAX_PROCESSES {
                    if j == i {
                        continue;
                    }
                    let o = &*slot_mut(j);
                    if o.used && o.cr3 == cr3 {
                        others = true;
                        break;
                    }
                }
            }
        }
        if !mm_shared || !others {
            crate::memory::free_mm(cr3);
        }
    }
    unsafe {
        *slot_mut(i) = Process::empty();
    }
}

/// Remove `child_pid` from parent slot's children list (compact).
pub fn remove_child(parent_idx: usize, child_pid: Pid) {
    if parent_idx >= MAX_PROCESSES {
        return;
    }
    unsafe {
        let p = &mut *slot_mut(parent_idx);
        let mut w = 0usize;
        let n = p.nchildren.min(super::pcb::MAX_CHILDREN);
        for r in 0..n {
            if p.children[r] != child_pid {
                p.children[w] = p.children[r];
                w += 1;
            }
        }
        p.nchildren = w;
        for i in w..super::pcb::MAX_CHILDREN {
            p.children[i] = 0;
        }
    }
}

pub fn init_child_slot(
    child_idx: usize,
    parent_pid: Pid,
    uid: Uid,
    stack_base: u64,
    stack_size: u64,
    heap_base: u64,
    heap_size: u64,
    is_thread: bool,
    out_pid: &mut Pid,
) {
    unsafe {
        let p = &mut *slot_mut(child_idx);
        *out_pid = p.pid;
        // Default: new process is its own thread group (fork). CLONE_THREAD overwrites.
        p.tgid = p.pid;
        p.parent = parent_pid;
        p.uid = uid;
        // Always Ready so the scheduler can pick the task. `is_thread` only
        // marks intent for name/debug; CLONE_THREAD sets tgid separately.
        let _ = is_thread;
        p.state = ProcessState::Ready;
        p.stack_base = stack_base;
        p.stack_size = stack_size;
        p.heap_base = heap_base;
        p.heap_size = heap_size;
        p.cwd_inode = 2;
        p.clear_child_tid = 0;
        p.mm_shared = false;
        // Default to parent CR3; fork overwrites with clone_mm result.
        p.cr3 = crate::memory::kernel_cr3();
        if let Some(parent_cr3) = with_pid(parent_pid, |par| par.cr3) {
            if parent_cr3 != 0 {
                p.cr3 = parent_cr3;
            }
        }
        p.trap_valid = false;
        p.kstack_top = super::kstack::top_for_slot(child_idx);
        p.set_name(if is_thread { "thread" } else { "child" });
    }
}

pub fn process_count() -> usize {
    let mut n = 0;
    unsafe {
        for i in 0..MAX_PROCESSES {
            if (*slot_mut(i)).used {
                n += 1;
            }
        }
    }
    n
}

pub fn for_each_process<F>(mut f: F)
where
    F: FnMut(usize, &Process),
{
    unsafe {
        for i in 0..MAX_PROCESSES {
            let p = &*slot_mut(i);
            if p.used {
                f(i, p);
            }
        }
    }
}

pub fn add_child(parent_idx: usize, child_pid: Pid) -> bool {
    unsafe {
        let p = &mut *slot_mut(parent_idx);
        if p.nchildren >= super::pcb::MAX_CHILDREN {
            return false;
        }
        p.children[p.nchildren] = child_pid;
        p.nchildren += 1;
        true
    }
}
