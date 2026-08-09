//! spinlock / mutex / completion / msleep (UP).

use crate::interrupts::timer;
use crate::interrupts::utils::enable_interrupts;

#[repr(C)]
pub struct SpinLock {
    pub locked: u32,
}

#[repr(C)]
pub struct Mutex {
    pub count: i32,
    pub wait_lock: SpinLock,
}

#[repr(C)]
pub struct Completion {
    pub done: u32,
}

fn save_flags_cli() -> u64 {
    let rflags: u64;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {f}",
            "cli",
            f = out(reg) rflags,
            options(nostack)
        );
    }
    rflags
}

fn restore_flags(flags: u64) {
    unsafe {
        core::arch::asm!(
            "push {f}",
            "popfq",
            f = in(reg) flags,
            options(nostack)
        );
    }
}

pub extern "C" fn spin_lock(lock: *mut SpinLock) {
    if lock.is_null() {
        return;
    }
    let _ = save_flags_cli();
    unsafe {
        (*lock).locked = 1;
    }
}

pub extern "C" fn spin_unlock(lock: *mut SpinLock) {
    if lock.is_null() {
        return;
    }
    unsafe {
        (*lock).locked = 0;
    }
    enable_interrupts();
}

pub extern "C" fn __spin_lock_irqsave(lock: *mut SpinLock) -> u64 {
    let flags = save_flags_cli();
    if !lock.is_null() {
        unsafe {
            (*lock).locked = 1;
        }
    }
    flags
}

pub extern "C" fn __spin_unlock_irqrestore(lock: *mut SpinLock, flags: u64) {
    if !lock.is_null() {
        unsafe {
            (*lock).locked = 0;
        }
    }
    restore_flags(flags);
}

pub extern "C" fn mutex_lock(m: *mut Mutex) {
    if m.is_null() {
        return;
    }
    loop {
        let flags = save_flags_cli();
        let c = unsafe { (*m).count };
        if c > 0 {
            unsafe {
                (*m).count = c - 1;
            }
            restore_flags(flags);
            return;
        }
        restore_flags(flags);
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

pub extern "C" fn mutex_unlock(m: *mut Mutex) {
    if m.is_null() {
        return;
    }
    let flags = save_flags_cli();
    unsafe {
        (*m).count = (*m).count.saturating_add(1);
    }
    restore_flags(flags);
}

pub extern "C" fn complete(x: *mut Completion) {
    if x.is_null() {
        return;
    }
    unsafe {
        let d = core::ptr::read_volatile(core::ptr::addr_of!((*x).done));
        core::ptr::write_volatile(core::ptr::addr_of_mut!((*x).done), d.wrapping_add(1));
    }
}

fn completion_done(x: *mut Completion) -> bool {
    if x.is_null() {
        return true;
    }
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*x).done)) > 0 }
}

pub extern "C" fn wait_for_completion(x: *mut Completion) {
    while !completion_done(x) {
        enable_interrupts();
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

/// `timeout` is in jiffies. Returns remaining jiffies, or 0 on timeout.
pub extern "C" fn wait_for_completion_timeout(x: *mut Completion, timeout: u64) -> u64 {
    let start = timer::ticks();
    loop {
        if completion_done(x) {
            let elapsed = timer::ticks().saturating_sub(start);
            return timeout.saturating_sub(elapsed);
        }
        if timer::ticks().saturating_sub(start) >= timeout {
            return 0;
        }
        enable_interrupts();
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

pub extern "C" fn msleep(msecs: u32) {
    let ticks_need = ((msecs as u64) + 9) / 10; // HZ=100
    let start = timer::ticks();
    while timer::ticks().saturating_sub(start) < ticks_need {
        enable_interrupts();
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}
