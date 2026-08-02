//! Futex wait/wake/requeue (Phase 6).
//!
//! Supports:
//! - `FUTEX_WAIT` / `FUTEX_WAKE` (+ `PRIVATE`)
//! - `FUTEX_REQUEUE` / `FUTEX_CMP_REQUEUE`
//! - `FUTEX_WAIT_BITSET` / `FUTEX_WAKE_BITSET` (bitset ignored; MATCH_ANY)
//!
//! Waiters are keyed by user virtual address; `FUTEX_PRIVATE` also keys by the
//! caller's CR3 so private futexes do not cross address spaces.
//!
//! The syscall layer runs Ready peers cooperatively while waiting, and may
//! `hlt` when idle until a peer is Ready, the word changes, or a timeout fires.

use super::pcb::ProcessState;
use super::table;

/// Max concurrent futex waiters system-wide.
const MAX_WAITERS: usize = 32;

#[derive(Clone, Copy)]
struct Waiter {
    used: bool,
    /// Task tid waiting.
    tid: i32,
    /// User futex word VA.
    uaddr: u64,
    /// Address space key: CR3 for PRIVATE, 0 for shared.
    mm_key: u64,
}

static mut WAITERS: [Waiter; MAX_WAITERS] = [Waiter {
    used: false,
    tid: 0,
    uaddr: 0,
    mm_key: 0,
}; MAX_WAITERS];

fn waiters_mut() -> &'static mut [Waiter; MAX_WAITERS] {
    unsafe { &mut *core::ptr::addr_of_mut!(WAITERS) }
}

fn current_mm_key(private: bool) -> u64 {
    if !private {
        return 0;
    }
    table::with_current(|p| {
        if p.cr3 != 0 {
            p.cr3
        } else {
            crate::memory::kernel_cr3()
        }
    })
    .unwrap_or(0)
}

fn enqueue(uaddr: u64, mm_key: u64, tid: i32) -> bool {
    let w = waiters_mut();
    for e in w.iter_mut() {
        if !e.used {
            e.used = true;
            e.tid = tid;
            e.uaddr = uaddr;
            e.mm_key = mm_key;
            return true;
        }
    }
    false
}

fn dequeue_tid(tid: i32) {
    let w = waiters_mut();
    for e in w.iter_mut() {
        if e.used && e.tid == tid {
            e.used = false;
            e.tid = 0;
            e.uaddr = 0;
            e.mm_key = 0;
        }
    }
}

/// Wake up to `n` waiters on `uaddr` (matching mm_key). Returns how many woke.
pub fn wake(uaddr: u64, n: u32, private: bool) -> u32 {
    if n == 0 {
        return 0;
    }
    let mm_key = current_mm_key(private);
    let w = waiters_mut();
    let mut woke = 0u32;
    for e in w.iter_mut() {
        if woke >= n {
            break;
        }
        if !e.used || e.uaddr != uaddr || e.mm_key != mm_key {
            continue;
        }
        let tid = e.tid;
        e.used = false;
        e.tid = 0;
        e.uaddr = 0;
        e.mm_key = 0;
        let _ = super::sched::wake_up(tid);
        woke += 1;
    }
    woke
}

/// `FUTEX_REQUEUE`: wake up to `nr_wake` on `uaddr`, move up to `nr_requeue`
/// remaining waiters from `uaddr` → `uaddr2`. Returns total tasks affected
/// (woken + requeued), matching Linux's approximate accounting.
pub fn requeue(uaddr: u64, uaddr2: u64, nr_wake: u32, nr_requeue: u32, private: bool) -> u32 {
    let mm_key = current_mm_key(private);
    let w = waiters_mut();
    let mut woke = 0u32;
    let mut moved = 0u32;

    // First pass: wake.
    for e in w.iter_mut() {
        if woke >= nr_wake {
            break;
        }
        if !e.used || e.uaddr != uaddr || e.mm_key != mm_key {
            continue;
        }
        let tid = e.tid;
        e.used = false;
        e.tid = 0;
        e.uaddr = 0;
        e.mm_key = 0;
        let _ = super::sched::wake_up(tid);
        woke += 1;
    }

    // Second pass: requeue remaining on uaddr to uaddr2.
    if nr_requeue > 0 && uaddr2 != 0 {
        for e in w.iter_mut() {
            if moved >= nr_requeue {
                break;
            }
            if !e.used || e.uaddr != uaddr || e.mm_key != mm_key {
                continue;
            }
            e.uaddr = uaddr2;
            moved += 1;
        }
    }

    woke.saturating_add(moved)
}

/// Enqueue current task as a futex waiter and mark Sleeping.
/// Returns `Err(-EAGAIN)` if `*uaddr != expected`, `Err(-ENOMEM)` if full.
pub fn begin_wait(uaddr: u64, expected: i32, private: bool) -> Result<(), i64> {
    let mm_key = current_mm_key(private);
    let me = table::current_pid();
    let cur = unsafe { core::ptr::read_volatile(uaddr as *const i32) };
    if cur != expected {
        return Err(-11); // EAGAIN
    }
    if !enqueue(uaddr, mm_key, me) {
        return Err(-12); // ENOMEM
    }
    super::sched::sleep_current();
    Ok(())
}

/// True if this tid is still on a futex wait queue.
pub fn still_waiting(tid: i32) -> bool {
    let w = waiters_mut();
    w.iter().any(|e| e.used && e.tid == tid)
}

/// Drop waiter entry without waking.
pub fn cancel_wait(tid: i32) {
    dequeue_tid(tid);
}

/// After cooperative schedule, make sure a woken waiter is Running if current.
pub fn ensure_running_if_current() {
    let _ = table::with_current(|p| {
        if p.state == ProcessState::Sleeping || p.state == ProcessState::Ready {
            p.state = ProcessState::Running;
        }
    });
}

/// Clear user clear_child_tid word and wake one waiter (pthread join).
///
/// # Safety
/// `ctid` must be a user `*mut i32` or 0.
pub unsafe fn clear_child_tid_and_wake(ctid: u64) {
    if ctid < 0x1000 || (ctid & 3) != 0 {
        return;
    }
    core::ptr::write_volatile(ctid as *mut i32, 0);
    // Joiner may use PRIVATE or shared op; wake both keys for this VA.
    let _ = wake(ctid, 1, true);
    let _ = wake(ctid, 1, false);
}
