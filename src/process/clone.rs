//! Linux-like `clone` — Phase 4 first slice.
//!
//! Supports a useful subset of flags for threads / shared-mm tasks:
//! - `CLONE_VM` — share parent CR3 (no private stack copy)
//! - `CLONE_THREAD` — same tgid as parent
//! - `CLONE_FILES` — share parent FD table (refcount)
//! - `CLONE_FS` — accepted (cwd still copied per-task for now)
//! - `CLONE_PARENT_SETTID` / `CLONE_CHILD_SETTID` / `CLONE_CHILD_CLEARTID`
//! - `CLONE_SETTLS` — set child `fs_base` from tls arg
//!
//! Without `CLONE_VM`, behaves like fork (private mm + stack copy) with an
//! optional alternate child stack pointer.

use super::fork::fork_from_user;
use super::pcb::ProcessState;
use super::table;
use super::trap::TrapFrame;

// Linux sched.h clone flags (subset).
pub const CLONE_VM: u64 = 0x0000_0100;
pub const CLONE_FS: u64 = 0x0000_0200;
pub const CLONE_FILES: u64 = 0x0000_0400;
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
pub const CLONE_THREAD: u64 = 0x0001_0000;
pub const CLONE_SETTLS: u64 = 0x0008_0000;
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;

/// Create a child task. Parent stays current; returns child **tid**.
///
/// `stack` is the child user RSP (0 → same as parent after fork-style copy).
/// `parent_tid` / `child_tid` are optional user `*int` for settid flags.
/// `tls` is the new FS base when `CLONE_SETTLS` is set.
pub fn clone_from_user(
    flags: u64,
    stack: u64,
    parent_tid: u64,
    child_tid: u64,
    tls: u64,
    user_rip: u64,
    user_rsp: u64,
    user_rflags: u64,
    child_trap: TrapFrame,
) -> Result<i32, i32> {
    // CSIGNAL low bits ignored (signal on death) for now.
    let _csignal = flags & 0xff;

    if flags & CLONE_VM == 0 {
        // Process-like: private mm via fork path, then fix stack/tid flags.
        let child = fork_from_user(user_rip, user_rsp, user_rflags)?;
        apply_clone_post(
            child,
            flags,
            stack,
            parent_tid,
            child_tid,
            tls,
            user_rip,
            user_rflags,
            child_trap,
        )?;
        return Ok(child);
    }

    // Shared address space.
    let parent_idx = table::current_index();
    let parent_pid = table::current_pid();
    let (
        uid,
        cwd,
        parent_cr3,
        parent_tgid,
        heap_base,
        heap_size,
        mmaps,
        mmap_bump,
        gs_base,
        dumpable,
        no_new_privs,
    ) = match table::with_current(|p| {
            let cr3 = if p.cr3 != 0 {
                p.cr3
            } else {
                crate::memory::kernel_cr3()
            };
            let tgid = if p.tgid != 0 { p.tgid } else { p.pid };
            (
                p.uid,
                p.cwd_inode,
                cr3,
                tgid,
                p.heap_base,
                p.heap_size,
                p.mmaps,
                p.mmap_bump,
                p.gs_base,
                p.dumpable,
                p.no_new_privs,
            )
        }) {
            Some(x) => x,
            None => return Err(-1),
        };

    // Mark parent mm shared too so free_index does not tear it down early.
    let _ = table::with_current(|p| {
        p.mm_shared = true;
    });

    let child_idx = match table::alloc_slot() {
        Some(i) => i,
        None => return Err(-1),
    };

    let is_thread = flags & CLONE_THREAD != 0;
    let mut child_pid = 0;
    table::init_child_slot(
        child_idx,
        parent_pid,
        uid,
        0,
        0,
        heap_base,
        heap_size,
        is_thread,
        &mut child_pid,
    );

    let child_rsp = if stack != 0 { stack } else { user_rsp };
    let child_fs = if flags & CLONE_SETTLS != 0 {
        tls
    } else {
        table::with_current(|p| p.fs_base).unwrap_or(0)
    };

    let _ = table::with_pid(child_pid, |p| {
        p.cwd_inode = cwd;
        p.cr3 = parent_cr3;
        p.mm_shared = true;
        p.tgid = if is_thread { parent_tgid } else { child_pid };
        p.state = ProcessState::Ready;
        p.fs_base = child_fs;
        p.gs_base = gs_base;
        p.user_rip = user_rip;
        p.user_rsp = child_rsp;
        p.user_rflags = user_rflags | 0x200;
        p.user_rax = 0;
        p.trap = child_trap;
        p.trap.rax = 0;
        p.trap.rsp = child_rsp;
        p.trap.rip = user_rip;
        p.trap.rflags = p.user_rflags;
        p.trap_valid = true;
        p.entered_via_nest = false;
        p.mmaps = mmaps;
        p.mmap_bump = mmap_bump;
        p.set_name(if is_thread { "thread" } else { "clonevm" });
        p.dumpable = dumpable;
        p.no_new_privs = no_new_privs;
        p.pdeathsig = 0;
        if flags & CLONE_CHILD_CLEARTID != 0 {
            p.clear_child_tid = child_tid;
        }
    });

    // FDs: share table when CLONE_FILES (typical with threads); else private copy.
    if flags & CLONE_FILES != 0 {
        crate::fd::share_table(parent_idx, child_idx);
    } else {
        crate::fd::clone_table(parent_idx, child_idx);
    }

    if !table::add_child(parent_idx, child_pid) {
        table::free_index(child_idx);
        return Err(-1);
    }

    // parent_tid / child_tid user stores
    write_tid_user(flags, parent_tid, child_tid, child_pid)?;

    Ok(child_pid)
}

fn apply_clone_post(
    child: i32,
    flags: u64,
    stack: u64,
    parent_tid: u64,
    child_tid: u64,
    tls: u64,
    user_rip: u64,
    user_rflags: u64,
    child_trap: TrapFrame,
) -> Result<(), i32> {
    let parent_tgid = table::with_current(|p| if p.tgid != 0 { p.tgid } else { p.pid }).unwrap_or(1);
    let parent_idx = table::current_index();
    let child_idx = table::find_pid(child).ok_or(-1)?;
    // fork_from_user already clone_table'd; upgrade to share if requested.
    if flags & CLONE_FILES != 0 {
        crate::fd::share_table(parent_idx, child_idx);
    }
    let _ = table::with_pid(child, |p| {
        if flags & CLONE_THREAD != 0 {
            p.tgid = parent_tgid;
            p.set_name("thread");
        }
        p.user_rip = user_rip;
        p.user_rflags = user_rflags | 0x200;
        p.trap = child_trap;
        p.trap.rax = 0;
        p.trap.rip = user_rip;
        p.trap.rflags = p.user_rflags;
        if stack != 0 {
            p.user_rsp = stack;
            p.trap.rsp = stack;
        }
        if flags & CLONE_SETTLS != 0 {
            p.fs_base = tls;
        }
        if flags & CLONE_CHILD_CLEARTID != 0 {
            p.clear_child_tid = child_tid;
        }
    });
    write_tid_user(flags, parent_tid, child_tid, child)
}

fn write_tid_user(flags: u64, parent_tid: u64, child_tid: u64, tid: i32) -> Result<(), i32> {
    if flags & CLONE_PARENT_SETTID != 0 && parent_tid != 0 {
        if !user_i32_ok(parent_tid) {
            return Err(-1);
        }
        unsafe {
            core::ptr::write_volatile(parent_tid as *mut i32, tid);
        }
    }
    if flags & CLONE_CHILD_SETTID != 0 && child_tid != 0 {
        if !user_i32_ok(child_tid) {
            return Err(-1);
        }
        unsafe {
            core::ptr::write_volatile(child_tid as *mut i32, tid);
        }
    }
    Ok(())
}

fn user_i32_ok(ptr: u64) -> bool {
    ptr >= 0x1000 && ptr.checked_add(4).is_some()
}
