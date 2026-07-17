//! Interrupt subsystem: IDT, PIC, exceptions, keyboard, timer, signals.

pub mod exceptions;
pub mod idt;
pub mod keyboard;
pub mod pic;
pub mod signal;
pub mod timer;
pub mod utils;

pub use idt::{
    idt_base, init_idt, is_registered as idt_is_registered, load_idt, present_gate_count,
    register_interrupt_handler,
};
pub use signal::sig;
pub use signal::{
    delivered_count, has_handler, has_pending, init_default_handlers, pending_count,
    process_signals, raise_signal, register_signal_handler, schedule_signal, signal_schedule,
    unregister_signal_handler, SignalCallback, MAX_SIGNALS,
};
pub use timer::{init_timer, ticks};
