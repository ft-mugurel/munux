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
        p.parent = -1;
        p.state = ProcessState::Running;
        p.uid = 0;
        p.stack_base = 0;
        p.stack_size = 0;
        p.heap_base = crate::memory::KERNEL_HEAP_START;
        p.heap_size = 0;
        p.cwd_inode = 2; // ext2 root inode
        // Kernel-side init (pid 1). Userspace /bin/sh is a child (U8 handoff).
        p.set_name("kinit");
        CURRENT = 0;
        NEXT_PID.store(2, Ordering::Relaxed);
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
    unsafe {
        *slot_mut(i) = Process::empty();
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
        p.parent = parent_pid;
        p.uid = uid;
        p.state = if is_thread {
            ProcessState::Thread
        } else {
            ProcessState::Ready
        };
        p.stack_base = stack_base;
        p.stack_size = stack_size;
        p.heap_base = heap_base;
        p.heap_size = heap_size;
        p.cwd_inode = 2;
        p.ctx.rsp = stack_base.wrapping_add(stack_size);
        p.ctx.rbp = p.ctx.rsp;
        p.set_name("child");
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
