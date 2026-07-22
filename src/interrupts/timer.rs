//! PIT — IRQ0 → vector 32.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::interrupts::idt::register_interrupt_handler;
use crate::interrupts::pic;
use crate::x86::io::outb;

const PIT_CH0: u16 = 0x40;
const PIT_CMD: u16 = 0x43;
const PIT_HZ: u32 = 1_193_182;
/// Programmed interrupt rate (also used for timekeeping).
pub const TARGET_HZ: u32 = 100;
/// Nanoseconds per PIT tick at `TARGET_HZ`.
pub const NS_PER_TICK: u64 = 1_000_000_000 / TARGET_HZ as u64;

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Monotonic nanoseconds since boot (from PIT ticks).
pub fn uptime_ns() -> u64 {
    ticks().saturating_mul(NS_PER_TICK)
}

pub fn init_timer() {
    let divisor = (PIT_HZ / TARGET_HZ) as u16;
    unsafe {
        outb(PIT_CMD, 0x36); // channel 0, lo/hi, mode 3
        outb(PIT_CH0, (divisor & 0xFF) as u8);
        outb(PIT_CH0, (divisor >> 8) as u8);
    }
    register_interrupt_handler(32, isr_timer);
}

#[no_mangle]
pub extern "C" fn timer_interrupt_handler() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    unsafe {
        pic::eoi_master();
    }
}

extern "C" {
    fn isr_timer();
}
