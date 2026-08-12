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

// ---------------------------------------------------------------------------
// P11a — console as a real tty (termios / winsize / session)
// ---------------------------------------------------------------------------

/// Linux kernel `struct termios` (TCGETS), NCCS=19 → 36 bytes.
pub const TERMIOS_LEN: usize = 36;
const NCCS: usize = 19;

// c_iflag / c_oflag / c_lflag / c_cflag (kernel octal bits)
pub const ICRNL: u32 = 0o0000400;
const IXON: u32 = 0o0002000;
pub const OPOST: u32 = 0o0000001;
pub const ONLCR: u32 = 0o0000004;
pub const ISIG: u32 = 0o0000001;
pub const ICANON: u32 = 0o0000002;
pub const ECHO: u32 = 0o0000010;
pub const ECHOE: u32 = 0o0000020;
pub const ECHOK: u32 = 0o0000040;
const IEXTEN: u32 = 0o100000;
const CS8: u32 = 0o0000060;
const CREAD: u32 = 0o0000200;
const HUPCL: u32 = 0o0002000;
const B38400: u32 = 0o0000017;

pub const CC_OFF: usize = 17;
pub const VINTR: usize = 0;
pub const VQUIT: usize = 1;
pub const VERASE: usize = 2;
pub const VKILL: usize = 3;
pub const VEOF: usize = 4;
const VTIME: usize = 5;
const VMIN: usize = 6;
const VSTART: usize = 8;
const VSTOP: usize = 9;
const VSUSP: usize = 10;

static CONSOLE_SID: AtomicI32 = AtomicI32::new(0);
static CONSOLE_FG_PGID: AtomicI32 = AtomicI32::new(1);
static mut CONSOLE_TERMIOS: [u8; TERMIOS_LEN] = [0; TERMIOS_LEN];
static TERMIOS_INIT: AtomicBool = AtomicBool::new(false);

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn fill_default_termios(out: &mut [u8; TERMIOS_LEN]) {
    default_termios(out);
}

fn default_termios(out: &mut [u8; TERMIOS_LEN]) {
    *out = [0; TERMIOS_LEN];
    put_u32(out, 0, ICRNL | IXON); // c_iflag
    put_u32(out, 4, OPOST | ONLCR); // c_oflag
    put_u32(out, 8, B38400 | CS8 | CREAD | HUPCL); // c_cflag
    put_u32(out, 12, ISIG | ICANON | ECHO | ECHOE | ECHOK | IEXTEN); // c_lflag
    out[16] = 0; // c_line
    let cc = CC_OFF;
    out[cc + VINTR] = 0x03;
    out[cc + VQUIT] = 0x1c;
    out[cc + VERASE] = 0x7f;
    out[cc + VKILL] = 0x15;
    out[cc + VEOF] = 0x04;
    out[cc + VTIME] = 0;
    out[cc + VMIN] = 1;
    out[cc + VSTART] = 0x11;
    out[cc + VSTOP] = 0x13;
    out[cc + VSUSP] = 0x1a;
    let _ = NCCS;
}

pub fn termios_u32(t: &[u8], off: usize) -> u32 {
    if off + 4 > t.len() {
        return 0;
    }
    u32::from_le_bytes([t[off], t[off + 1], t[off + 2], t[off + 3]])
}

pub fn termios_cc(t: &[u8], idx: usize) -> u8 {
    let i = CC_OFF + idx;
    if i < t.len() {
        t[i]
    } else {
        0
    }
}

fn ensure_termios() {
    if TERMIOS_INIT.swap(true, Ordering::Relaxed) {
        return;
    }
    unsafe {
        default_termios(&mut *core::ptr::addr_of_mut!(CONSOLE_TERMIOS));
    }
}

pub fn console_get_termios(out: &mut [u8]) {
    ensure_termios();
    let n = out.len().min(TERMIOS_LEN);
    unsafe {
        out[..n].copy_from_slice(&CONSOLE_TERMIOS[..n]);
    }
}

pub fn console_set_termios(src: &[u8]) {
    ensure_termios();
    let n = src.len().min(TERMIOS_LEN);
    unsafe {
        CONSOLE_TERMIOS[..n].copy_from_slice(&src[..n]);
    }
}

pub fn console_winsize() -> (u16, u16) {
    (25, 80) // rows, cols (VGA text)
}

pub fn console_fg_pgid() -> i32 {
    CONSOLE_FG_PGID.load(Ordering::Relaxed)
}

pub fn set_console_fg_pgid(pgid: i32) {
    if pgid > 0 {
        CONSOLE_FG_PGID.store(pgid, Ordering::Relaxed);
    }
}

pub fn console_sid() -> i32 {
    CONSOLE_SID.load(Ordering::Relaxed)
}

/// Attach console as controlling tty of this session. Returns 0 or -errno.
pub fn tiocsctty(steal: bool) -> i32 {
    let (pid, sid, pgid, ctty) = crate::process::with_current(|p| (p.pid, p.sid, p.pgid, p.ctty))
        .unwrap_or((0, 0, 0, 0));
    if pid <= 0 || pid != sid {
        return -1; // EPERM: not session leader
    }
    if ctty != 0 {
        return 0; // already ours
    }
    let owner = CONSOLE_SID.load(Ordering::Relaxed);
    if owner != 0 && owner != sid && !steal {
        return -1; // EPERM
    }
    CONSOLE_SID.store(sid, Ordering::Relaxed);
    CONSOLE_FG_PGID.store(if pgid != 0 { pgid } else { sid }, Ordering::Relaxed);
    let tgid = crate::process::getpid();
    for i in 0..crate::process::pcb::MAX_PROCESSES {
        let _ = crate::process::table::with_index(i, |p| {
            if p.used && p.tgid == tgid {
                p.ctty = 1;
            }
        });
    }
    0
}

pub fn tiocnotty() -> i32 {
    let (sid, ctty) = crate::process::with_current(|p| (p.sid, p.ctty)).unwrap_or((0, 0));
    if ctty == 0 {
        return -25; // ENOTTY
    }
    let tgid = crate::process::getpid();
    for i in 0..crate::process::pcb::MAX_PROCESSES {
        let _ = crate::process::table::with_index(i, |p| {
            if p.used && p.tgid == tgid {
                p.ctty = 0;
            }
        });
    }
    if CONSOLE_SID.load(Ordering::Relaxed) == sid {
        CONSOLE_SID.store(0, Ordering::Relaxed);
    }
    if let Some(n) = crate::fs::pty::index_from_ctty(ctty) {
        crate::fs::pty::set_sid(n, 0);
    }
    0
}

/// Attach PTY `n` as controlling tty. Returns 0 or -errno.
pub fn tiocsctty_pty(n: usize, steal: bool) -> i32 {
    let (pid, sid, pgid, ctty) = crate::process::with_current(|p| (p.pid, p.sid, p.pgid, p.ctty))
        .unwrap_or((0, 0, 0, 0));
    if pid <= 0 || pid != sid {
        return -1;
    }
    if ctty != 0 {
        return 0;
    }
    if !crate::fs::pty::is_used(n) {
        return -25;
    }
    let owner = crate::fs::pty::sid(n);
    if owner != 0 && owner != sid && !steal {
        return -1;
    }
    crate::fs::pty::set_sid(n, sid);
    crate::fs::pty::set_fg_pgid(n, if pgid != 0 { pgid } else { sid });
    let tgid = crate::process::getpid();
    let mark = crate::fs::pty::ctty_for(n);
    for i in 0..crate::process::pcb::MAX_PROCESSES {
        let _ = crate::process::table::with_index(i, |p| {
            if p.used && p.tgid == tgid {
                p.ctty = mark;
            }
        });
    }
    0
}
