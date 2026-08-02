//! Minimal TTY / job-control hooks for Ctrl-C (SIGINT).
//!
//! - Remember who is blocked in console `read` (foreground tgid).
//! - Keyboard IRQ only sets a pending flag (no process teardown in IRQ).
//! - Timer tick / explicit poll delivers SIGINT to the foreground (or current
//!   user task as fallback).

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::process::pcb::Pid;
use crate::process::signal_queue::{self, SIGINT};

/// Last process group (tgid) that entered a blocking console read; 0 = none.
static FG_TGID: AtomicI32 = AtomicI32::new(0);
/// Ctrl-C seen in keyboard IRQ; drained on tick / console wait loop.
static PENDING_SIGINT: AtomicBool = AtomicBool::new(false);

/// Call when a task begins blocking console input.
pub fn enter_console_read() {
    let tgid = crate::process::getpid(); // getpid returns tgid
    if tgid > 1 {
        FG_TGID.store(tgid, Ordering::Relaxed);
    }
}

/// Call when console read finishes (optional; leave FG for “last reader”).
pub fn leave_console_read() {
    // Keep FG_TGID so Ctrl-C after a short read still targets the shell/app.
}

/// Keyboard IRQ: request SIGINT (do not deliver here).
pub fn request_sigint_from_irq() {
    PENDING_SIGINT.store(true, Ordering::Relaxed);
}

/// True if a Ctrl-C is waiting to be delivered.
pub fn sigint_pending() -> bool {
    PENDING_SIGINT.load(Ordering::Relaxed)
}

/// Deliver pending TTY SIGINT if any.
///
/// Target priority (important for `sh` wait + child job):
/// 1. **Current** user task tgid — who was interrupted / has the CPU
///    (the running job, e.g. `busybox sleep`)
/// 2. Else foreground console-reader tgid (shell blocked in `read`)
///
/// If the target is the current task, only set `force_fatal_sig` (no
/// `exit_user` from IRQ). Process context must poll and call
/// [`crate::syscalls::fatal_signal_exit`].
pub fn deliver_pending_sigint() {
    if !PENDING_SIGINT.swap(false, Ordering::Relaxed) {
        return;
    }
    let cur_tid = crate::process::gettid();
    let cur_tgid = crate::process::getpid();
    let fg = FG_TGID.load(Ordering::Relaxed);

    // Prefer the active job (current user process), not the last tty reader.
    // Otherwise Ctrl-C while `sh` waits kills the shell, not the child.
    let target = if cur_tid > 1 && cur_tgid > 1 {
        cur_tgid
    } else if fg > 1 {
        fg
    } else {
        return;
    };

    if cur_tgid == target && cur_tid > 1 {
        // Current task is in the target group (often mid-syscall like nanosleep).
        signal_queue::mark_group_fatal(target as Pid, SIGINT);
    } else {
        // Other process — safe to tear down from tick context.
        let _ = signal_queue::proc_kill(target as Pid, SIGINT);
    }
}

/// Process-context: if this task has a deferred fatal signal, take it (clear).
/// Caller should invoke the full exit path (nest-safe).
pub fn take_force_fatal() -> Option<u32> {
    let sig = crate::process::with_current(|p| {
        let s = p.force_fatal_sig;
        if s != 0 {
            p.force_fatal_sig = 0;
        }
        s
    })
    .unwrap_or(0);
    if sig == 0 {
        None
    } else {
        Some(sig)
    }
}

/// For tests: set foreground without a real read.
#[allow(dead_code)]
pub fn set_foreground_tgid(tgid: Pid) {
    FG_TGID.store(tgid, Ordering::Relaxed);
}
