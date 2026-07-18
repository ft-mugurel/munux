//! Unix-like helpers: wait, exit, getuid, signal, kill, user-task spawn.

use super::pcb::{Pid, ProcessState, Uid};
use super::signal_queue;
use super::table;

pub fn getuid() -> Uid {
    table::with_current(|p| p.uid).unwrap_or(0)
}

pub fn setuid(uid: Uid) -> i32 {
    table::with_current(|p| {
        p.uid = uid;
        0
    })
    .unwrap_or(-1)
}

pub fn getpid() -> Pid {
    table::current_pid()
}

pub fn getppid() -> Pid {
    table::with_current(|p| p.parent).unwrap_or(-1)
}

pub fn kill(pid: Pid, sig: u32) -> i32 {
    signal_queue::proc_kill(pid, sig)
}

pub fn signal(sig: u32, handler: usize) -> usize {
    signal_queue::proc_signal(sig, handler)
}

/// Terminate current process as zombie and switch to parent.
///
/// For cooperative user tasks: after this returns, the kernel calls
/// `return_from_user` so the shell/`run` launcher resumes as the parent.
pub fn exit_user(status: i32) {
    let pid = table::current_pid();
    let parent = table::with_current(|p| {
        p.exit_code = status & 0xff;
        p.state = ProcessState::Zombie;
        p.parent
    })
    .unwrap_or(1);

    if pid == 1 {
        crate::console::println("fatal: init (pid 1) exited");
        loop {
            unsafe {
                core::arch::asm!("cli; hlt", options(nomem, nostack));
            }
        }
    }

    if parent > 0 {
        if let Some(i) = table::find_pid(parent) {
            table::set_current_index(i);
            let _ = table::with_pid(parent, |p| {
                if p.state != ProcessState::Zombie {
                    p.state = ProcessState::Running;
                }
            });
            return;
        }
    }
    // Orphan: reparent to init and switch there
    if let Some(i) = table::find_pid(1) {
        let _ = table::with_pid(pid, |p| {
            p.parent = 1;
        });
        let _ = table::add_child(i, pid);
        table::set_current_index(i);
        let _ = table::with_pid(1, |p| {
            if p.state != ProcessState::Zombie {
                p.state = ProcessState::Running;
            }
        });
    }
}

/// Legacy cooperative exit — marks zombie then parks (kernel paths only).
pub fn exit(status: i32) -> ! {
    exit_user(status);
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

/// Wait for a zombie child (non-blocking). Returns child pid or -1.
pub fn wait(status_out: Option<&mut i32>) -> Pid {
    waitpid(-1, status_out, false)
}

/// waitpid(pid, status): pid == -1 → any child.
/// If `nohang` and no zombie, returns 0. If no children at all, returns -1 (ECHILD).
pub fn waitpid(wait_for: Pid, status_out: Option<&mut i32>, nohang: bool) -> Pid {
    let parent = table::current_pid();

    // Do we have any matching children (alive or zombie)?
    let mut has_child = false;
    table::for_each_process(|_idx, p| {
        if p.used && p.parent == parent && (wait_for == -1 || wait_for == 0 || p.pid == wait_for)
        {
            has_child = true;
        }
    });
    if !has_child {
        return -1; // ECHILD
    }

    let mut found_pid: Pid = -1;
    let mut found_code = 0;
    let mut found_idx = 0usize;

    table::for_each_process(|idx, p| {
        if found_pid != -1 {
            return;
        }
        if p.used
            && p.state == ProcessState::Zombie
            && p.parent == parent
            && (wait_for == -1 || wait_for == 0 || p.pid == wait_for)
        {
            found_pid = p.pid;
            found_code = p.exit_code;
            found_idx = idx;
        }
    });

    if found_pid < 0 {
        // No scheduler sleep yet: behave like WNOHANG when children exist.
        let _ = nohang;
        return if has_child { 0 } else { -1 };
    }
    if let Some(s) = status_out {
        // Linux wait status: normal exit → (code & 0xff) << 8
        *s = (found_code & 0xff) << 8;
    }
    // Remove from parent child list
    let _ = table::with_pid(parent, |p| {
        let mut w = 0;
        for r in 0..p.nchildren {
            if p.children[r] != found_pid {
                p.children[w] = p.children[r];
                w += 1;
            }
        }
        p.nchildren = w;
    });
    table::free_index(found_idx);
    found_pid
}

/// Spawn a user child of the current process and switch current → child.
/// Used by kernel `run` / `user` (not full fork yet — U6).
pub fn begin_user_task(name: &str) -> Result<Pid, i32> {
    let parent_idx = table::current_index();
    let parent_pid = table::current_pid();

    let (uid, cwd, heap_base, heap_size) = match table::with_current(|p| {
        (p.uid, p.cwd_inode, p.heap_base, p.heap_size)
    }) {
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

    let _ = table::with_pid(child_pid, |p| {
        p.cwd_inode = cwd;
        p.state = ProcessState::Running;
        p.set_name(name);
    });

    if !table::add_child(parent_idx, child_pid) {
        table::free_index(child_idx);
        return Err(-1);
    }

    // Parent waits (Sleeping) until child exits and we switch back
    let _ = table::with_pid(parent_pid, |p| {
        if p.state == ProcessState::Running {
            p.state = ProcessState::Sleeping;
        }
    });
    table::set_current_index(child_idx);
    Ok(child_pid)
}

/// After a user task returns to the kernel launcher: reap the zombie child.
/// Returns (pid, raw exit code) if one was reaped.
pub fn reap_any_child() -> Option<(Pid, i32)> {
    let mut status = 0i32;
    let pid = waitpid(-1, Some(&mut status), true);
    if pid > 0 {
        // Decode Linux wait status back to raw exit code for shell messages
        let code = (status >> 8) & 0xff;
        Some((pid, code))
    } else {
        None
    }
}
