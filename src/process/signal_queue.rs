//! Per-process signal queue; delivery on CPU tick.

use super::pcb::{Pid, ProcessState, MAX_SIGNALS};
use super::table;

pub fn proc_kill(pid: Pid, sig: u32) -> i32 {
    if sig == 0 {
        return if table::find_pid(pid).is_some() { 0 } else { -1 };
    }
    if (sig as usize) >= MAX_SIGNALS {
        return -1;
    }
    table::with_pid(pid, |p| {
        if p.sig_ignore[sig as usize] {
            return 0;
        }
        if p.push_signal(sig) {
            0
        } else {
            -1
        }
    })
    .unwrap_or(-1)
}

pub fn proc_signal(sig: u32, handler: usize) -> usize {
    if (sig as usize) >= MAX_SIGNALS {
        return usize::MAX;
    }
    table::with_current(|p| {
        let old = p.sig_handlers[sig as usize];
        if handler == 1 {
            p.sig_ignore[sig as usize] = true;
            p.sig_handlers[sig as usize] = 0;
        } else {
            p.sig_ignore[sig as usize] = false;
            p.sig_handlers[sig as usize] = handler;
        }
        old
    })
    .unwrap_or(usize::MAX)
}

pub fn deliver_pending_on_tick() {
    let mut pids = [-1i32; super::pcb::MAX_PROCESSES];
    let mut n = 0;
    table::for_each_process(|_i, p| {
        if n < pids.len() {
            pids[n] = p.pid;
            n += 1;
        }
    });
    for i in 0..n {
        let pid = pids[i];
        if pid < 0 {
            continue;
        }
        let _ = table::with_pid(pid, |p| {
            while let Some(sig) = p.pop_signal() {
                if (sig as usize) >= MAX_SIGNALS {
                    continue;
                }
                if p.sig_ignore[sig as usize] {
                    continue;
                }
                let h = p.sig_handlers[sig as usize];
                if h != 0 {
                    let f: fn(u32) = unsafe { core::mem::transmute(h) };
                    f(sig);
                } else if sig == 9 || sig == 15 {
                    // default: terminate
                    if p.pid != 1 {
                        p.exit_code = 128 + sig as i32;
                        p.state = ProcessState::Zombie;
                    }
                }
            }
        });
    }
}
