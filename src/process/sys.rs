//! Unix-like helpers: wait, exit, getuid, signal, kill.

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

pub fn kill(pid: Pid, sig: u32) -> i32 {
    signal_queue::proc_kill(pid, sig)
}

pub fn signal(sig: u32, handler: usize) -> usize {
    signal_queue::proc_signal(sig, handler)
}

/// Terminate current process (zombie). Does not return to a normal parent
/// context in this cooperative kernel — marks zombie and switches to init.
pub fn exit(status: i32) -> ! {
    let pid = table::current_pid();
    let _ = table::with_pid(pid, |p| {
        p.exit_code = status;
        p.state = ProcessState::Zombie;
    });
    if pid == 1 {
        crate::panic::halt_clean("init (pid 1) exited");
    }
    if let Some(i) = table::find_pid(1) {
        table::set_current_index(i);
        let _ = table::with_pid(1, |p| {
            if p.state != ProcessState::Zombie {
                p.state = ProcessState::Running;
            }
        });
    }
    loop {
        crate::interrupts::signal::process_signals();
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

/// Wait for a zombie child (non-blocking). Returns child pid or -1.
/// Unix `wait` can block; shell should retry or use this non-blocking form.
pub fn wait(status_out: Option<&mut i32>) -> Pid {
    waitpid(-1, status_out)
}

/// waitpid(pid, status): pid == -1 → any child.
pub fn waitpid(wait_for: Pid, status_out: Option<&mut i32>) -> Pid {
    let parent = table::current_pid();
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
            && (wait_for == -1 || p.pid == wait_for)
        {
            found_pid = p.pid;
            found_code = p.exit_code;
            found_idx = idx;
        }
    });

    if found_pid < 0 {
        return -1;
    }
    if let Some(s) = status_out {
        *s = found_code;
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
