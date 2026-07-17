//! PIT — IRQ0 → vector 32. CPU ticks for process signal delivery.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::interrupts::idt::register_interrupt_handler;
use crate::x86::io::outb;

const PIT_CH0: u16 = 0x40;
const PIT_CMD: u16 = 0x43;
const PIT_HZ: u32 = 1193182;
const TARGET_HZ: u32 = 100;

static TICKS: AtomicU32 = AtomicU32::new(0);

pub fn ticks() -> u32 {
    TICKS.load(Ordering::Relaxed)
}

pub fn init_timer() {
    let divisor = (PIT_HZ / TARGET_HZ) as u16;
    unsafe {
        outb(PIT_CMD, 0x36);
        outb(PIT_CH0, (divisor & 0xFF) as u8);
        outb(PIT_CH0, (divisor >> 8) as u8);
    }
    register_interrupt_handler(32, isr_timer);
}

#[no_mangle]
pub extern "C" fn timer_interrupt_handler() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::process::on_cpu_tick();
    unsafe {
        outb(0x20, 0x20);
    }
}

extern "C" {
    fn isr_timer();
}
