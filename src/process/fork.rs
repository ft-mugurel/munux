//! fork — copy process (Unix-like teaching implementation; full fork is U6).

use super::memory::alloc_process_stack;
use super::pcb::ProcessState;
use super::table;

/// Fork current process. Parent returns child PID (>0); -1 on error.
///
/// Child PCB is a copy of parent metadata with a new stack and Ready state.
/// Shares the kernel address space (no separate page directory yet).
pub fn fork() -> i32 {
    let parent_idx = table::current_index();
    let parent_pid = table::current_pid();

    let (uid, heap_base, heap_size, cwd) =
        match table::with_current(|p| (p.uid, p.heap_base, p.heap_size, p.cwd_inode)) {
            Some(x) => x,
            None => return -1,
        };

    let child_idx = match table::alloc_slot() {
        Some(i) => i,
        None => return -1,
    };

    let (stack_base, stack_size) = match alloc_process_stack(8 * 1024) {
        Some(x) => x,
        None => {
            table::free_index(child_idx);
            return -1;
        }
    };

    let mut child_pid = 0;
    table::init_child_slot(
        child_idx,
        parent_pid,
        uid,
        stack_base,
        stack_size,
        heap_base,
        heap_size,
        false,
        &mut child_pid,
    );
    let _ = table::with_pid(child_pid, |p| {
        p.cwd_inode = cwd;
    });

    if !table::add_child(parent_idx, child_pid) {
        table::free_index(child_idx);
        return -1;
    }

    unsafe {
        core::ptr::write_bytes(stack_base as *mut u8, 0, stack_size as usize);
    }

    child_pid
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
