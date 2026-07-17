//! Kernel signal / callback API.
//!
//! A **signal** is a small numbered event the kernel can raise.
//! A **callback** is a function you register to run when that signal is delivered.
//!
//! Delivery is **deferred**: interrupts/exceptions call [`schedule_signal`];
//! the main loop (or shell) calls [`process_signals`] so work is not done deep
//! inside an ISR when possible.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Maximum distinct signal numbers we support (0..MAX_SIGNALS).
pub const MAX_SIGNALS: usize = 32;

/// Pending queue length (ring buffer of signal numbers).
const QUEUE_CAP: usize = 32;

/// Built-in signal numbers (kernel API).
pub mod sig {
    pub const NONE: u32 = 0;
    /// Timer / periodic tick (when a timer IRQ exists).
    pub const ALARM: u32 = 1;
    /// Keyboard activity (optional notify).
    pub const KEYBOARD: u32 = 2;
    /// Page fault observed (before panic path may halt).
    pub const PAGEFAULT: u32 = 14;
    /// General protection fault.
    pub const GPF: u32 = 13;
    /// Generic kernel warning (non-fatal).
    pub const WARNING: u32 = 20;
    /// Request soft shutdown / halt preparation.
    pub const TERM: u32 = 15;
    /// User-defined range starts here.
    pub const USER0: u32 = 24;
}

/// Callback type: receives the signal number.
pub type SignalCallback = fn(sig: u32);

#[derive(Clone, Copy)]
struct Slot {
    used: bool,
    cb: Option<SignalCallback>,
}

static mut HANDLERS: [Slot; MAX_SIGNALS] = [Slot {
    used: false,
    cb: None,
}; MAX_SIGNALS];

static mut QUEUE: [u32; QUEUE_CAP] = [0; QUEUE_CAP];
static mut Q_HEAD: usize = 0;
static mut Q_TAIL: usize = 0;
static mut Q_LEN: usize = 0;

/// True if at least one signal is waiting.
static PENDING: AtomicBool = AtomicBool::new(false);

/// Total signals delivered (for debugging).
static DELIVERED: AtomicU32 = AtomicU32::new(0);

/// Register (or replace) a callback for `sig`.
///
/// Returns `false` if `sig` is out of range.
pub fn register_signal_handler(sig: u32, callback: SignalCallback) -> bool {
    let i = sig as usize;
    if i >= MAX_SIGNALS {
        return false;
    }
    unsafe {
        HANDLERS[i].used = true;
        HANDLERS[i].cb = Some(callback);
    }
    true
}

/// Remove a handler for `sig`.
pub fn unregister_signal_handler(sig: u32) -> bool {
    let i = sig as usize;
    if i >= MAX_SIGNALS {
        return false;
    }
    unsafe {
        HANDLERS[i].used = false;
        HANDLERS[i].cb = None;
    }
    true
}

/// Schedule a signal to be delivered later (safe-ish from interrupt context).
///
/// Does **not** run the callback immediately — use [`process_signals`].
pub fn schedule_signal(sig: u32) -> bool {
    if sig as usize >= MAX_SIGNALS || sig == sig::NONE {
        return false;
    }
    unsafe {
        if Q_LEN >= QUEUE_CAP {
            return false; // drop if full
        }
        QUEUE[Q_TAIL] = sig;
        Q_TAIL = (Q_TAIL + 1) % QUEUE_CAP;
        Q_LEN += 1;
    }
    PENDING.store(true, Ordering::SeqCst);
    true
}

/// Alias for the subject wording: "interface to schedule signals".
#[inline]
pub fn signal_schedule(sig: u32) -> bool {
    schedule_signal(sig)
}

/// How many signals are waiting in the queue.
pub fn pending_count() -> usize {
    unsafe { Q_LEN }
}

pub fn has_pending() -> bool {
    PENDING.load(Ordering::SeqCst)
}

/// Deliver all pending signals (run their callbacks).
///
/// Call this from the main kernel loop (not from deep nested IRQs if possible).
pub fn process_signals() {
    loop {
        let sig = unsafe { dequeue() };
        let Some(sig) = sig else {
            PENDING.store(false, Ordering::SeqCst);
            break;
        };
        let cb = unsafe {
            let i = sig as usize;
            if i < MAX_SIGNALS && HANDLERS[i].used {
                HANDLERS[i].cb
            } else {
                None
            }
        };
        if let Some(f) = cb {
            f(sig);
            DELIVERED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

unsafe fn dequeue() -> Option<u32> {
    if Q_LEN == 0 {
        return None;
    }
    let sig = QUEUE[Q_HEAD];
    Q_HEAD = (Q_HEAD + 1) % QUEUE_CAP;
    Q_LEN -= 1;
    Some(sig)
}

/// Raise and process immediately (for non-ISR kernel code).
pub fn raise_signal(sig: u32) {
    let _ = schedule_signal(sig);
    process_signals();
}

pub fn delivered_count() -> u32 {
    DELIVERED.load(Ordering::Relaxed)
}

/// Whether a handler is registered for `sig`.
pub fn has_handler(sig: u32) -> bool {
    let i = sig as usize;
    if i >= MAX_SIGNALS {
        return false;
    }
    unsafe { HANDLERS[i].used && HANDLERS[i].cb.is_some() }
}

/// Default handlers installed at boot (logging only; non-fatal signals).
pub fn init_default_handlers() {
    let _ = register_signal_handler(sig::WARNING, default_warning);
    let _ = register_signal_handler(sig::KEYBOARD, default_keyboard_signal);
    let _ = register_signal_handler(sig::TERM, default_term);
}

fn default_warning(sig: u32) {
    crate::println!("[signal] WARNING ({})", sig);
}

fn default_keyboard_signal(_sig: u32) {
    // Quiet by default — keyboard already prints characters.
    // Left as a hook for future “Ctrl+C → SIGINT” style behavior.
}

fn default_term(_sig: u32) {
    crate::println!("[signal] TERM — requesting halt");
    crate::panic::halt_clean("signal: TERM");
}
