//! Scheduler (Phase 3c): cooperative wait path + **IRQ preemption**.
//!
//! - Pick **Ready** tasks (round-robin)
//! - `sleep_current` / `wake_up` / `take_ready` (wait path)
//! - Timer IRQ: if interrupted in **user** mode and another task is Ready,
//!   save [`TrapFrame`] to the current PCB, switch CR3/TLS, load next frame
//!   onto the IRQ stack so `iretq` resumes the other task.
//!
//! Cooperative nest enter (wait) still uses [`UserFrame`] + `enter_user_mode`.

use super::fork::UserFrame;
use super::pcb::{ProcessState, MAX_PROCESSES};
use super::table;
use super::trap::TrapFrame;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Set by the timer; consumed by preemption / schedule paths.
static NEED_RESCHED: AtomicBool = AtomicBool::new(false);

/// How many times IRQ preemption actually switched tasks.
static PREEMPT_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Round-robin cursor over process table slots.
static RR_CURSOR: AtomicUsize = AtomicUsize::new(0);

const RESCHED_EVERY_TICKS: u64 = 1;

pub fn on_timer_tick(ticks: u64) {
    if ticks % RESCHED_EVERY_TICKS == 0 {
        NEED_RESCHED.store(true, Ordering::Relaxed);
    }
}

pub fn need_resched() -> bool {
    NEED_RESCHED.load(Ordering::Relaxed)
}

pub fn clear_need_resched() {
    NEED_RESCHED.store(false, Ordering::Relaxed);
}

/// Number of successful IRQ task switches (for `munux> preempt`).
pub fn preempt_count() -> u64 {
    PREEMPT_COUNT.load(Ordering::Relaxed)
}

/// Reset the IRQ preemption counter (before a focused test).
pub fn reset_preempt_count() {
    PREEMPT_COUNT.store(0, Ordering::Relaxed);
}

/// When true, `try_preempt` runs even if `need_resched` is clear (unit tests).
static FORCE_PREEMPT_TEST: AtomicBool = AtomicBool::new(false);

pub fn set_force_preempt_test(on: bool) {
    FORCE_PREEMPT_TEST.store(on, Ordering::Relaxed);
}

/// Unit-test helper: simulate an IRQ preempt with a caller-owned frame.
///
/// # Safety
/// `frame` must be a valid mutable TrapFrame (user CS).
pub unsafe fn test_try_preempt(frame: *mut TrapFrame) {
    NEED_RESCHED.store(true, Ordering::Relaxed);
    FORCE_PREEMPT_TEST.store(true, Ordering::Relaxed);
    try_preempt(frame);
    FORCE_PREEMPT_TEST.store(false, Ordering::Relaxed);
}

pub fn sleep_current() {
    let _ = table::with_current(|p| {
        if p.state == ProcessState::Running {
            p.state = ProcessState::Sleeping;
        }
    });
}

pub fn wake_up(pid: i32) -> bool {
    table::with_pid(pid, |p| {
        if !p.used {
            return false;
        }
        if p.state == ProcessState::Sleeping || p.state == ProcessState::Ready {
            p.state = ProcessState::Ready;
            true
        } else {
            false
        }
    })
    .unwrap_or(false)
}

/// Pick next Ready process (RR). Optional prefer pid. Skips `skip` pid if set.
pub fn pick_ready(prefer: Option<i32>) -> Option<(i32, UserFrame)> {
    pick_ready_skip(prefer, None)
}

fn pick_ready_skip(prefer: Option<i32>, skip: Option<i32>) -> Option<(i32, UserFrame)> {
    if let Some(pid) = prefer {
        if skip != Some(pid) {
            if let Some(frame) = frame_if_ready(pid) {
                return Some((pid, frame));
            }
        }
    }

    let start = RR_CURSOR.load(Ordering::Relaxed) % MAX_PROCESSES;
    for off in 0..MAX_PROCESSES {
        let idx = (start + off) % MAX_PROCESSES;
        let mut found: Option<(i32, UserFrame)> = None;
        table::for_each_process(|i, p| {
            if found.is_some() || i != idx {
                return;
            }
            if p.used && p.state == ProcessState::Ready && skip != Some(p.pid) {
                found = Some((p.pid, user_frame_from_process(p)));
            }
        });
        if let Some(pair) = found {
            RR_CURSOR.store((idx + 1) % MAX_PROCESSES, Ordering::Relaxed);
            return Some(pair);
        }
    }
    None
}

fn user_frame_from_process(p: &super::pcb::Process) -> UserFrame {
    if p.trap_valid {
        UserFrame {
            rip: p.trap.rip,
            rsp: p.trap.rsp,
            rflags: p.trap.rflags,
            rax: p.trap.rax,
        }
    } else {
        UserFrame {
            rip: p.user_rip,
            rsp: p.user_rsp,
            rflags: p.user_rflags,
            rax: p.user_rax,
        }
    }
}

fn frame_if_ready(pid: i32) -> Option<UserFrame> {
    table::with_pid(pid, |p| {
        if p.used && p.state == ProcessState::Ready {
            Some(user_frame_from_process(p))
        } else {
            None
        }
    })
    .flatten()
}

pub fn switch_current_to(pid: i32, sleep_prev: bool) -> bool {
    let cur = table::current_pid();
    if cur == pid {
        let _ = table::with_pid(pid, |p| p.state = ProcessState::Running);
        return true;
    }
    let Some(idx) = table::find_pid(pid) else {
        return false;
    };
    let _ = table::with_pid(cur, |p| {
        if p.state == ProcessState::Running {
            p.state = if sleep_prev {
                ProcessState::Sleeping
            } else {
                ProcessState::Ready
            };
        }
    });
    table::set_current_index(idx);
    let _ = table::with_pid(pid, |p| p.state = ProcessState::Running);
    true
}

pub fn take_ready(prefer: i32) -> Option<UserFrame> {
    let parent = table::current_pid();
    let (pid, frame) = if prefer > 0 {
        pick_ready(Some(prefer))?
    } else {
        let mut child_pid = -1i32;
        table::for_each_process(|_i, p| {
            if child_pid >= 0 {
                return;
            }
            if p.used && p.state == ProcessState::Ready && p.parent == parent {
                child_pid = p.pid;
            }
        });
        if child_pid > 0 {
            pick_ready(Some(child_pid))?
        } else {
            pick_ready(None)?
        }
    };

    if !switch_current_to(pid, true) {
        return None;
    }
    Some(frame)
}

pub fn has_ready_child(prefer: i32) -> bool {
    let parent = table::current_pid();
    let mut found = false;
    table::for_each_process(|_i, p| {
        if found || !p.used || p.state != ProcessState::Ready {
            return;
        }
        if prefer > 0 {
            if p.pid == prefer {
                found = true;
            }
        } else if p.parent == parent {
            found = true;
        }
    });
    found
}

// ---------------------------------------------------------------------------
// IRQ preemption
// ---------------------------------------------------------------------------

/// Called from the timer ISR with a pointer to the interrupt stack frame.
///
/// If we interrupted **user** mode and another process is Ready, rewrite
/// `frame` so `iretq` returns into that process.
///
/// Nest-safe policy (Phase 3e):
/// - IRQ uses per-process kstack (TSS.RSP0), **not** the nest syscall stack.
/// - `entered_via_nest` is **sticky** (never cleared here).
/// - Preempt allowed at nest depth 0 or 1 (top-level `run` / boot shell child
///   dual-spin). Depth ≥ 2 means wait/exec nesting — stay cooperative there
///   so IRQ cannot race with an inner `return_from_user` chain (was #UD).
///
/// # Safety
/// `frame` must point at a live IRQ stack layout matching [`TrapFrame`].
pub unsafe fn try_preempt(frame: *mut TrapFrame) {
    if frame.is_null() {
        return;
    }
    let f = &mut *frame;
    if !f.is_user() {
        return; // interrupted kernel — do not switch
    }
    // Only switch when the timer asked (or a unit test forced a switch).
    if !need_resched() && !FORCE_PREEMPT_TEST.load(Ordering::Relaxed) {
        return;
    }

    // Depth ≥ 2: forktest/busybox under shell wait — cooperative only.
    // Depth 0–1: munux> preempttest dual-spin (and top-level run) may IRQ-switch.
    if !FORCE_PREEMPT_TEST.load(Ordering::Relaxed) {
        extern "C" {
            fn get_enter_nest_depth() -> u64;
        }
        if unsafe { get_enter_nest_depth() } > 1 {
            return;
        }
    }

    let cur = table::current_pid();
    // Need another Ready task (not the one we interrupted).
    let Some((next_pid, _)) = pick_ready_skip(None, Some(cur)) else {
        clear_need_resched();
        return;
    };

    clear_need_resched();

    // Save interrupted user context onto the current PCB.
    // Keep entered_via_nest as-is (still owns a nest frame if it had one).
    let _ = table::with_current(|p| {
        p.trap = *f;
        p.trap_valid = true;
        p.user_rip = f.rip;
        p.user_rsp = f.rsp;
        p.user_rflags = f.rflags;
        p.user_rax = f.rax;
    });

    // cur Running → Ready; next → Running + CR3 / kstack / TLS
    // (`set_current_index` inside switch installs mm + stacks).
    if !switch_current_to(next_pid, false) {
        return;
    }

    // Load next task onto the IRQ stack for iretq.
    // Do **not** clear entered_via_nest: if this task was enter_user_mode'd,
    // it still owns a nest frame until return_from_user on its real exit.
    let mut ok = false;
    let _ = table::with_current(|p| {
        let next = if p.trap_valid {
            p.trap
        } else {
            let t = TrapFrame::from_user_entry(p.user_rip, p.user_rsp, p.user_rflags, p.user_rax);
            p.trap = t;
            p.trap_valid = true;
            t
        };
        // Refuse to iretq into garbage (kernel CS / null rip → #GP/#UD).
        if next.is_user() && next.rip >= 0x1000 {
            *f = next;
            ok = true;
        }
    });
    if !ok {
        // Roll back: stay on previous task if next frame was unusable.
        // (Rare — Ready tasks should always have a user entry.)
        let _ = switch_current_to(cur, false);
        let _ = table::with_current(|p| {
            if p.trap_valid {
                *f = p.trap;
            }
            p.state = ProcessState::Running;
        });
        return;
    }
    PREEMPT_COUNT.fetch_add(1, Ordering::Relaxed);
}
