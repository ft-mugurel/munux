//! Signals (Phase 5 first slice).
//!
//! - Queue pending signals per task
//! - `kill` / `tkill` / `tgkill`
//! - Default action: terminate (except ignored / blocked)
//! - `SIG_IGN` / `SIG_DFL` via handlers table
//! - User-mode handlers: stored but not yet framed on the user stack
//!   (default terminate covers roadmap exit criteria for kill)

use super::pcb::{Pid, ProcessState, MAX_SIGNALS};
use super::table;

/// Linux signal numbers we care about.
pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGKILL: u32 = 9;
pub const SIGTERM: u32 = 15;
pub const SIGCHLD: u32 = 17;
pub const SIGSTOP: u32 = 19;

/// Handler sentinels (Linux).
pub const SIG_DFL: u64 = 0;
pub const SIG_IGN: u64 = 1;

fn valid_sig(sig: u32) -> bool {
    sig > 0 && (sig as usize) < MAX_SIGNALS
}

/// Signals that cannot be caught or ignored.
fn is_uncatchable(sig: u32) -> bool {
    sig == SIGKILL || sig == SIGSTOP
}

/// Default terminate signals (others default to ignore for this slice).
fn default_terminates(sig: u32) -> bool {
    matches!(
        sig,
        SIGHUP | SIGINT | SIGQUIT | SIGKILL | SIGTERM | 6 | 4 | 8 | 11
        // ABRT=6, ILL=4, FPE=8, SEGV=11
    )
}

fn is_blocked(p: &super::pcb::Process, sig: u32) -> bool {
    if is_uncatchable(sig) {
        return false;
    }
    if sig == 0 || sig >= 64 {
        return false;
    }
    (p.sig_blocked & (1u64 << (sig as u64))) != 0
}

/// Existence check / queue / default-terminate.
///
/// `pid` is a **tid** for thread-directed, or we also match **tgid** for
/// process-directed `kill`.
pub fn proc_kill(pid: Pid, sig: u32) -> i32 {
    if pid <= 0 {
        return -3; // ESRCH (pgrp form is proc_kill_pgrp)
    }
    if sig == 0 {
        return if find_task(pid).is_some() { 0 } else { -3 };
    }
    if !valid_sig(sig) {
        return -22; // EINVAL
    }

    // Prefer exact tid; else any task with tgid == pid (process-directed).
    let target = match find_task(pid) {
        Some(t) => t,
        None => return -3,
    };

    // Process-directed kill: act on the whole thread group for fatal defaults.
    let tgid = target.1;
    if default_terminates(sig) && !is_ignored_group(tgid, sig) && !is_handled_group(tgid, sig) {
        return terminate_group(tgid, sig);
    }

    // Queue on the specific task (or leader if only tgid matched).
    queue_or_ignore(target.0, sig)
}

/// `kill(-pgid, sig)` / `kill(0, sig)` — every process in that group.
pub fn proc_kill_pgrp(pgid: Pid, sig: u32) -> i32 {
    if pgid <= 0 {
        return -22;
    }
    let mut tgids = [0i32; super::pcb::MAX_PROCESSES];
    let mut n = 0usize;
    table::for_each_process(|_, p| {
        if !p.used || p.pgid != pgid || p.state == ProcessState::Zombie {
            return;
        }
        let tg = if p.tgid != 0 { p.tgid } else { p.pid };
        let mut seen = false;
        for t in tgids.iter().take(n) {
            if *t == tg {
                seen = true;
                break;
            }
        }
        if !seen && n < tgids.len() {
            tgids[n] = tg;
            n += 1;
        }
    });
    if n == 0 {
        return -3;
    }
    if sig == 0 {
        return 0;
    }
    if !valid_sig(sig) {
        return -22;
    }
    let mut last = 0i32;
    for t in tgids.iter().take(n) {
        last = proc_kill(*t, sig);
    }
    last
}

/// Thread-directed: must match exact tid.
pub fn proc_tkill(tid: Pid, sig: u32) -> i32 {
    if tid <= 0 {
        return -3;
    }
    if sig == 0 {
        return if table::find_pid(tid).is_some() { 0 } else { -3 };
    }
    if !valid_sig(sig) {
        return -22;
    }
    if table::find_pid(tid).is_none() {
        return -3;
    }

    if default_terminates(sig) {
        let ign = table::with_pid(tid, |p| p.sig_ignore[sig as usize] && !is_uncatchable(sig))
            .unwrap_or(false);
        let handled = table::with_pid(tid, |p| {
            p.sig_handlers[sig as usize] != 0
                && p.sig_handlers[sig as usize] != SIG_IGN as usize
                && !is_uncatchable(sig)
        })
        .unwrap_or(false);
        if !ign && !handled {
            // Terminate whole group (thread death of fatal = process death for now).
            let tgid = table::with_pid(tid, |p| if p.tgid != 0 { p.tgid } else { p.pid })
                .unwrap_or(tid);
            return terminate_group(tgid, sig);
        }
    }
    queue_or_ignore(tid, sig)
}

pub fn proc_tgkill(tgid: Pid, tid: Pid, sig: u32) -> i32 {
    if tid <= 0 || tgid <= 0 {
        return -3;
    }
    let ok = table::with_pid(tid, |p| {
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        g == tgid
    })
    .unwrap_or(false);
    if !ok {
        return -3;
    }
    proc_tkill(tid, sig)
}

/// Returns (tid, tgid) if found by tid or as group leader tgid.
fn find_task(pid: Pid) -> Option<(Pid, Pid)> {
    if let Some(t) = table::with_pid(pid, |p| {
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        (p.pid, g)
    }) {
        return Some(t);
    }
    // Match as tgid (process id).
    let mut found: Option<(Pid, Pid)> = None;
    table::for_each_process(|_i, p| {
        if found.is_some() || !p.used {
            return;
        }
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        if g == pid {
            found = Some((p.pid, g));
        }
    });
    found
}

fn is_ignored_group(tgid: Pid, sig: u32) -> bool {
    if is_uncatchable(sig) {
        return false;
    }
    let mut ign = false;
    table::for_each_process(|_i, p| {
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        if g == tgid && p.sig_ignore[sig as usize] {
            ign = true;
        }
    });
    ign
}

fn is_handled_group(tgid: Pid, sig: u32) -> bool {
    if is_uncatchable(sig) {
        return false;
    }
    let mut h = false;
    table::for_each_process(|_i, p| {
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        if g == tgid {
            let hh = p.sig_handlers[sig as usize];
            if hh != 0 && hh != SIG_IGN as usize {
                h = true;
            }
        }
    });
    h
}

fn queue_or_ignore(tid: Pid, sig: u32) -> i32 {
    // If the target has a user handler and is not the current task, inject
    // delivery onto its saved trap now — a spinning child never makes a
    // syscall, so post-syscall delivery alone is not enough.
    let cur = table::current_pid();
    let injected = table::with_pid(tid, |p| {
        if is_blocked(p, sig) {
            return Ok(false);
        }
        if p.sig_ignore[sig as usize] && !is_uncatchable(sig) {
            return Ok(false);
        }
        let h = p.sig_handlers[sig as usize];
        if tid != cur
            && h != 0
            && h != SIG_IGN as usize
            && !is_uncatchable(sig)
            && !p.sig_in_handler
        {
            let restore = if p.trap_valid {
                p.trap
            } else {
                super::trap::TrapFrame::from_user_entry(
                    p.user_rip,
                    p.user_rsp,
                    p.user_rflags,
                    p.user_rax,
                )
            };
            let tcr3 = if p.cr3 != 0 {
                p.cr3
            } else {
                crate::memory::kernel_cr3()
            };
            if let Some(hframe) = build_handler_frame(&restore, h as u64, sig, tcr3) {
                p.sig_restore = restore;
                p.sig_in_handler = true;
                p.trap = hframe;
                p.trap_valid = true;
                p.user_rip = hframe.rip;
                p.user_rsp = hframe.rsp;
                p.user_rax = hframe.rax;
                p.user_rflags = hframe.rflags;
                return Ok(true);
            }
        }
        if p.push_signal(sig) {
            Ok(false)
        } else {
            Err(-11)
        }
    });
    match injected {
        Some(Ok(_)) => 0,
        Some(Err(e)) => e,
        None => -3,
    }
}

/// Terminate all tasks in a thread group; leave one zombie for the parent.
fn terminate_group(tgid: Pid, sig: u32) -> i32 {
    let exit_code = (128 + (sig as i32)) & 0xff;
    // Collect tids in group.
    let mut tids = [-1i32; super::pcb::MAX_PROCESSES];
    let mut n = 0usize;
    let mut parent = 1i32;
    let mut leader = tgid;
    table::for_each_process(|_i, p| {
        if !p.used {
            return;
        }
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        if g != tgid {
            return;
        }
        if n < tids.len() {
            tids[n] = p.pid;
            n += 1;
        }
        if p.pid == tgid || p.tgid == p.pid {
            leader = p.pid;
            parent = p.parent;
        }
    });
    if n == 0 {
        return -3;
    }

    // Prefer leader as the surviving zombie.
    let mut zombie_tid = leader;
    if table::find_pid(zombie_tid).is_none() {
        zombie_tid = tids[0];
    }

    for i in 0..n {
        let tid = tids[i];
        if tid <= 0 || tid == zombie_tid {
            continue;
        }
        // Do not kill init.
        if tid == 1 {
            continue;
        }
        if let Some(idx) = table::find_pid(tid) {
            let ctid = table::with_index(idx, |p| {
                let t = p.clear_child_tid;
                p.clear_child_tid = 0;
                t
            })
            .unwrap_or(0);
            if ctid != 0 {
                unsafe {
                    super::futex::clear_child_tid_and_wake(ctid);
                }
            }
            table::free_index(idx);
        }
    }

    // Zombie the remaining task.
    if zombie_tid == 1 {
        return 0; // refuse to kill init
    }
    let cur = table::current_pid();
    if zombie_tid == cur {
        // Self-kill: use normal exit path so nest return works.
        // Status: Linux wait encodes signal death specially; we use exit code.
        super::sys::exit_user(exit_code);
        return 0;
    }

    let _ = table::with_pid(zombie_tid, |p| {
        p.exit_code = exit_code;
        p.state = ProcessState::Zombie;
        let ctid = p.clear_child_tid;
        p.clear_child_tid = 0;
        if ctid != 0 {
            unsafe {
                super::futex::clear_child_tid_and_wake(ctid);
            }
        }
    });
    if parent > 0 {
        let _ = super::sched::wake_up(parent);
    }
    0
}

/// Classic `signal()` / helper for rt_sigaction.
/// Returns previous handler address, or `usize::MAX` on error.
pub fn proc_signal(sig: u32, handler: usize) -> usize {
    if !valid_sig(sig) || is_uncatchable(sig) {
        return usize::MAX;
    }
    table::with_current(|p| {
        let old = p.sig_handlers[sig as usize];
        if handler == SIG_IGN as usize {
            p.sig_ignore[sig as usize] = true;
            p.sig_handlers[sig as usize] = SIG_IGN as usize;
        } else if handler == SIG_DFL as usize {
            p.sig_ignore[sig as usize] = false;
            p.sig_handlers[sig as usize] = 0;
        } else {
            p.sig_ignore[sig as usize] = false;
            p.sig_handlers[sig as usize] = handler;
        }
        old
    })
    .unwrap_or(usize::MAX)
}

/// Apply a new signal mask (low 64 signals). Returns previous mask.
///
/// Linux `how`: SIG_BLOCK=0, SIG_UNBLOCK=1, SIG_SETMASK=2.
pub fn proc_sigprocmask(how: u32, set: Option<u64>) -> u64 {
    table::with_current(|p| {
        let old = p.sig_blocked;
        if let Some(s) = set {
            let s = s & !((1u64 << SIGKILL) | (1u64 << SIGSTOP));
            match how {
                0 => p.sig_blocked |= s,  // SIG_BLOCK
                1 => p.sig_blocked &= !s, // SIG_UNBLOCK
                2 => p.sig_blocked = s,   // SIG_SETMASK
                _ => {}
            }
        }
        old
    })
    .unwrap_or(0)
}

/// Mark every live task in `tgid` with a deferred fatal signal (IRQ-safe-ish).
/// Skips tasks that ignore the signal (e.g. interactive shell SIG_IGN SIGINT).
pub fn mark_group_fatal(tgid: Pid, sig: u32) {
    if !valid_sig(sig) {
        return;
    }
    let mut tids = [-1i32; super::pcb::MAX_PROCESSES];
    let mut n = 0usize;
    table::for_each_process(|_i, p| {
        if !p.used {
            return;
        }
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        if g == tgid && p.pid != 1 && n < tids.len() {
            if p.sig_ignore[sig as usize] && !is_uncatchable(sig) {
                return;
            }
            // User handler: do not force-kill; queue for delivery instead.
            let h = p.sig_handlers[sig as usize];
            if h != 0 && h != SIG_IGN as usize && !is_uncatchable(sig) {
                return;
            }
            tids[n] = p.pid;
            n += 1;
        }
    });
    for i in 0..n {
        let tid = tids[i];
        if tid > 0 {
            let _ = table::with_pid(tid, |p| {
                p.force_fatal_sig = sig;
            });
        }
    }
}

/// User VA for shared `rt_sigreturn` trampoline (mapped RX into each mm on demand).
pub const SIG_RESTORER_VA: u64 = 0x0000_0000_7ffd_0000;

/// Ensure `cr3` has a user-executable restorer page:
/// `mov rax, 15; syscall` (Linux `rt_sigreturn`).
pub fn ensure_restorer_mapped_in(cr3: u64) {
    use crate::memory::paging::{self, PAGE_PRESENT, PAGE_USER};
    use crate::memory::pmm;

    let cr3 = if cr3 != 0 {
        cr3
    } else {
        crate::memory::kernel_cr3()
    };

    if paging::virt_to_phys_in(cr3, SIG_RESTORER_VA).is_some() {
        return;
    }
    let Some(frame) = pmm::alloc_frame() else {
        return;
    };
    // mov rax, 15; syscall
    let code: [u8; 9] = [
        0x48, 0xC7, 0xC0, 0x0F, 0x00, 0x00, 0x00, // mov rax, 15
        0x0F, 0x05, // syscall
    ];
    unsafe {
        let dst = frame.as_u64() as *mut u8;
        core::ptr::write_bytes(dst, 0xCC, 4096);
        core::ptr::copy_nonoverlapping(code.as_ptr(), dst, code.len());
    }
    // Present + User, not Writable (RX).
    paging::map_page_in(cr3, SIG_RESTORER_VA, frame, PAGE_PRESENT | PAGE_USER);
}

pub fn ensure_restorer_mapped() {
    let cr3 = table::with_current(|p| {
        if p.cr3 != 0 {
            p.cr3
        } else {
            crate::memory::kernel_cr3()
        }
    })
    .unwrap_or_else(crate::memory::kernel_cr3);
    ensure_restorer_mapped_in(cr3);
}

/// Build a handler entry frame from a saved user context.
///
/// Stack layout (downward): `[rsp] = restorer` so handler `ret` → rt_sigreturn.
/// System V: `rdi = sig`.
///
/// `target_cr3` is the address space that owns the user stack (may not be current).
fn build_handler_frame(
    restore: &super::trap::TrapFrame,
    handler: u64,
    sig: u32,
    target_cr3: u64,
) -> Option<super::trap::TrapFrame> {
    ensure_restorer_mapped_in(target_cr3);
    let mut rsp = restore.rsp;
    // Align and push restorer return address.
    rsp = rsp.saturating_sub(8) & !0xFu64;
    if rsp < 0x1000 {
        return None;
    }
    // Write restorer pointer into the **target** address space.
    let cr3 = if target_cr3 != 0 {
        target_cr3
    } else {
        crate::memory::kernel_cr3()
    };
    let phys = crate::memory::paging::virt_to_phys_in(cr3, rsp)?;
    let page_off = (rsp & 0xfff) as usize;
    unsafe {
        let dst = ((phys & !0xfff) as *mut u8).add(page_off) as *mut u64;
        core::ptr::write_volatile(dst, SIG_RESTORER_VA);
    }
    let mut h = *restore;
    h.rip = handler;
    h.rsp = rsp;
    h.rdi = sig as u64;
    h.rax = 0;
    h.rflags |= 0x200;
    Some(h)
}

/// Result of trying to deliver one pending signal to the current task.
pub enum DeliverResult {
    /// Nothing to do.
    None,
    /// Enter this frame as the signal handler (does not return via sysret).
    Handler(super::trap::TrapFrame),
    /// Fatal default — caller should terminate (already may have switched).
    Fatal(u32),
}

/// Deliver one pending signal for the **current** task (process context).
///
/// `restore` is the user context to resume after the handler (`rt_sigreturn`).
pub fn try_deliver_one(restore: &super::trap::TrapFrame) -> DeliverResult {
    let me = table::current_pid();
    if me <= 1 {
        return DeliverResult::None;
    }
    // Do not nest handlers in this slice.
    if table::with_current(|p| p.sig_in_handler).unwrap_or(false) {
        return DeliverResult::None;
    }

    let mut fatal: Option<u32> = None;
    let mut handler_pair: Option<(u32, u64)> = None;

    let _ = table::with_current(|p| {
        while let Some(sig) = p.pop_signal() {
            if !valid_sig(sig) {
                continue;
            }
            if is_blocked(p, sig) {
                continue;
            }
            if p.sig_ignore[sig as usize] && !is_uncatchable(sig) {
                continue;
            }
            let h = p.sig_handlers[sig as usize];
            if h != 0 && h != SIG_IGN as usize && !is_uncatchable(sig) {
                handler_pair = Some((sig, h as u64));
                break;
            }
            if default_terminates(sig) {
                fatal = Some(sig);
                break;
            }
        }
    });

    if let Some(sig) = fatal {
        return DeliverResult::Fatal(sig);
    }
    if let Some((sig, handler)) = handler_pair {
        let tcr3 = table::with_current(|p| {
            if p.cr3 != 0 {
                p.cr3
            } else {
                crate::memory::kernel_cr3()
            }
        })
        .unwrap_or_else(crate::memory::kernel_cr3);
        if let Some(frame) = build_handler_frame(restore, handler, sig, tcr3) {
            let _ = table::with_current(|p| {
                p.sig_restore = *restore;
                p.sig_in_handler = true;
            });
            return DeliverResult::Handler(frame);
        }
    }
    DeliverResult::None
}

/// Legacy name: process fatal-only delivery (used from older call sites).
pub fn deliver_pending_current() {
    // Used when we have no restore frame (e.g. early in dispatch). Only fatals.
    let me = table::current_pid();
    if me <= 1 {
        return;
    }
    let mut fatal: Option<u32> = None;
    let _ = table::with_current(|p| {
        // Peek without consuming non-fatal: only pop fatals if no handler.
        // Simpler: pop all and re-queue non-fatal is hard — only check force path.
        while let Some(sig) = p.pop_signal() {
            if !valid_sig(sig) {
                continue;
            }
            if is_blocked(p, sig) || (p.sig_ignore[sig as usize] && !is_uncatchable(sig)) {
                continue;
            }
            let h = p.sig_handlers[sig as usize];
            if h != 0 && h != SIG_IGN as usize && !is_uncatchable(sig) {
                // Put back by push — order may reverse; OK for one signal.
                let _ = p.push_signal(sig);
                break;
            }
            if default_terminates(sig) {
                fatal = Some(sig);
                break;
            }
        }
    });
    if let Some(sig) = fatal {
        let _ = terminate_group(
            table::with_current(|p| if p.tgid != 0 { p.tgid } else { p.pid }).unwrap_or(me),
            sig,
        );
    }
}

/// `rt_sigreturn`: restore user context saved at handler entry.
pub fn do_sigreturn() -> Option<super::trap::TrapFrame> {
    table::with_current(|p| {
        if !p.sig_in_handler {
            return None;
        }
        p.sig_in_handler = false;
        let f = p.sig_restore;
        p.sig_restore = super::trap::TrapFrame::zero();
        if f.rip >= 0x1000 && f.is_user() {
            Some(f)
        } else {
            None
        }
    })
    .flatten()
}

/// Tick path: only process non-current tasks' fatal defaults if queued.
/// Avoids calling exit from IRQ for the running task.
pub fn deliver_pending_on_tick() {
    let cur = table::current_pid();
    let mut pids = [-1i32; super::pcb::MAX_PROCESSES];
    let mut n = 0;
    table::for_each_process(|_i, p| {
        if n < pids.len() && p.used && p.sig_len > 0 {
            pids[n] = p.pid;
            n += 1;
        }
    });
    for i in 0..n {
        let pid = pids[i];
        if pid < 0 || pid == cur {
            continue;
        }
        let mut fatal = None;
        let _ = table::with_pid(pid, |p| {
            while let Some(sig) = p.pop_signal() {
                if p.sig_ignore[sig as usize] && !is_uncatchable(sig) {
                    continue;
                }
                if is_blocked(p, sig) {
                    continue;
                }
                let h = p.sig_handlers[sig as usize];
                if h != 0 && h != SIG_IGN as usize && !is_uncatchable(sig) {
                    continue;
                }
                if default_terminates(sig) {
                    fatal = Some(sig);
                    break;
                }
            }
        });
        if let Some(sig) = fatal {
            let tgid = table::with_pid(pid, |p| if p.tgid != 0 { p.tgid } else { p.pid })
                .unwrap_or(pid);
            let _ = terminate_group(tgid, sig);
        }
    }
}
