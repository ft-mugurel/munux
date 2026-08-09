//! Unix-like helpers: waitpid, exit_user, getuid/getpid, user-task spawn.

use super::pcb::{Pid, ProcessState, Uid};
use super::table;

pub fn getuid() -> Uid {
    table::with_current(|p| p.uid).unwrap_or(0)
}

/// Linux `getpid` — returns **tgid** (thread-group id).
pub fn getpid() -> Pid {
    table::with_current(|p| {
        if p.tgid != 0 {
            p.tgid
        } else {
            p.pid
        }
    })
    .unwrap_or(1)
}

/// Linux `gettid` — returns unique task id.
pub fn gettid() -> Pid {
    table::current_pid()
}

pub fn getppid() -> Pid {
    table::with_current(|p| p.parent).unwrap_or(-1)
}

/// Deliver each child's `PR_SET_PDEATHSIG` when `dying` (this task) exits.
fn notify_pdeath(dying: Pid) {
    let mut kids = [(-1i32, 0u32); super::pcb::MAX_PROCESSES];
    let mut n = 0usize;
    table::for_each_process(|_i, p| {
        if !p.used || p.pid == dying {
            return;
        }
        if p.parent == dying && p.pdeathsig != 0 && p.state != ProcessState::Zombie {
            if n < kids.len() {
                kids[n] = (p.pid, p.pdeathsig);
                n += 1;
            }
        }
    });
    for i in 0..n {
        let (tid, sig) = kids[i];
        if tid > 0 && sig != 0 {
            // Queue only — do not `proc_kill`/`terminate_group` from inside
            // `exit_user` (nested exit smashes the dying task's nest frame).
            let _ = table::with_pid(tid, |p| {
                let _ = p.push_signal(sig);
                p.force_fatal_sig = sig;
            });
            let _ = crate::process::sched::wake_up(tid);
        }
    }
}

/// Terminate current process as zombie and switch to parent.
///
/// For cooperative user tasks: after this returns, the kernel calls
/// `return_from_user` so the shell/`run` launcher resumes as the parent.
///
/// Non-leader threads (`pid != tgid`) are **auto-reaped** (freed) after the
/// `clear_child_tid` futex wake — join uses futex, not `wait4`. Zombie threads
/// would leak PCB slots and `children[]` entries across multi-clone tests.
pub fn exit_user(status: i32) {
    let pid = table::current_pid();
    // Phase 6: clear_child_tid + futex wake (musl pthread join).
    let ctid = table::with_current(|p| {
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

    let (tgid, parent) = table::with_current(|p| {
        let tgid = if p.tgid != 0 { p.tgid } else { p.pid };
        p.exit_code = status & 0xff;
        // Mark zombie first so wait paths see us; free below if non-leader.
        p.state = ProcessState::Zombie;
        (tgid, p.parent)
    })
    .unwrap_or((pid, 1));

    // Linux prctl(PR_SET_PDEATHSIG): children of this task get the signal now.
    notify_pdeath(pid);

    if pid == 1 {
        crate::console::println("fatal: init (pid 1) exited");
        loop {
            unsafe {
                core::arch::asm!("cli; hlt", options(nomem, nostack));
            }
        }
    }

    // Switch to parent (or init) before freeing self.
    let mut switched = false;
    if parent > 0 {
        if let Some(i) = table::find_pid(parent) {
            let _ = crate::process::sched::wake_up(parent);
            table::set_current_index(i);
            let _ = table::with_pid(parent, |p| {
                if p.state != ProcessState::Zombie {
                    p.state = ProcessState::Running;
                }
            });
            switched = true;
        }
    }
    if !switched {
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

    // Auto-reap non-leader threads (no waitable status).
    if pid != tgid {
        if let Some(idx) = table::find_pid(pid) {
            table::free_index(idx);
        }
    }
}

/// Linux `exit_group`: tear down every task in the current thread group, then
/// zombie the caller for the parent (one waitable status).
///
/// Sibling threads are freed immediately (not left as extra zombies). Shared
/// mm / FD tables drop one ref each via [`table::free_index`].
pub fn exit_group(status: i32) {
    let me = table::current_pid();
    let tgid = table::with_current(|p| {
        if p.tgid != 0 {
            p.tgid
        } else {
            p.pid
        }
    })
    .unwrap_or(me);

    // Collect sibling tids (same tgid, not me).
    let mut kill = [-1i32; super::pcb::MAX_PROCESSES];
    let mut n = 0usize;
    table::for_each_process(|_i, p| {
        if !p.used || p.pid == me {
            return;
        }
        let g = if p.tgid != 0 { p.tgid } else { p.pid };
        if g == tgid && n < kill.len() {
            kill[n] = p.pid;
            n += 1;
        }
    });

    for i in 0..n {
        let tid = kill[i];
        if tid <= 0 {
            continue;
        }
        if let Some(idx) = table::find_pid(tid) {
            // Clear sibling clear_child_tid before free (joiners may wait).
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
            // Do not leave a waitable zombie for peer threads.
            table::free_index(idx);
        }
    }

    // Remaining task in the group becomes the zombie the parent waits on.
    exit_user(status);
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

    let (uid, cwd) = match table::with_current(|p| (p.uid, p.cwd_inode)) {
        Some(x) => x,
        None => return Err(-1),
    };

    let child_idx = match table::alloc_slot() {
        Some(i) => i,
        None => return Err(-1),
    };

    let mut child_pid = 0;
    // Fresh user image: heap is set from ELF brk_start before enter_user_mode.
    // Do not inherit kinit's kernel heap VA.
    table::init_child_slot(
        child_idx,
        parent_pid,
        uid,
        0,
        0,
        0,
        0,
        false,
        &mut child_pid,
    );

    // Inherit open FDs from the launcher (kinit / shell).
    crate::fd::clone_table(parent_idx, child_idx);

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
