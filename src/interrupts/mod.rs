//! Interrupt subsystem (x86_64): IDT + CPU exceptions.

pub mod exceptions;
pub mod idt;

pub use idt::{init_idt, present_gate_count, register_interrupt_handler};
pub use exceptions::init_exceptions;
