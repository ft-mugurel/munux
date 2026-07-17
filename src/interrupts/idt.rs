//! Interrupt Descriptor Table (IDT).
//!
//! The IDT tells the CPU: "when interrupt/exception number N happens,
//! jump to this function (gate)."
//!
//! Flow:
//! 1. Create a table of 256 gates in memory (filled with handlers).
//! 2. Register it with the CPU using `lidt`.

use core::arch::asm;
use core::ptr::addr_of;

use crate::gdt::gdt::KERNEL_CODE_SELECTOR;

/// 32-bit interrupt gate: Present, DPL=0, type=0xE.
const GATE_INTERRUPT_32: u8 = 0x8E;

/// One IDT gate (8 bytes), hardware layout.
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    zero: u8,
    flags: u8,
    offset_high: u16,
}

impl IdtEntry {
    const fn empty() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            zero: 0,
            flags: 0, // not present
            offset_high: 0,
        }
    }

    fn from_handler(handler: unsafe extern "C" fn()) -> Self {
        Self::from_handler_flags(handler, GATE_INTERRUPT_32)
    }

    fn from_handler_flags(handler: unsafe extern "C" fn(), flags: u8) -> Self {
        let addr = handler as u32;
        Self {
            offset_low: addr as u16,
            selector: KERNEL_CODE_SELECTOR,
            zero: 0,
            flags,
            offset_high: (addr >> 16) as u16,
        }
    }
}

/// GDTR-style pointer: limit + base (6 bytes).
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u32,
}

/// The live IDT (256 possible vectors: 0–31 exceptions, 32+ IRQs, …).
static mut IDT: [IdtEntry; 256] = [IdtEntry::empty(); 256];

static mut IDT_REGISTERED: bool = false;

/// Create / reset the IDT (all entries not-present), then **register** it with `lidt`.
///
/// Call this early in boot. Handlers are installed afterwards with
/// [`register_interrupt_handler`] / [`fill_and_register`] paths.
pub fn init_idt() {
    unsafe {
        for i in 0..256 {
            IDT[i] = IdtEntry::empty();
        }
    }
    load_idt();
    unsafe {
        IDT_REGISTERED = true;
    }
}

/// Load the current IDT table into the CPU (IDTR).
pub fn load_idt() {
    let idt_ptr = IdtPointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: addr_of!(IDT) as u32,
    };
    unsafe {
        asm!(
            "lidt [{}]",
            in(reg) &idt_ptr,
            options(nostack, preserves_flags)
        );
    }
}

/// Install one handler at vector `index` (0–255) and keep IDTR pointing at our table.
pub fn register_interrupt_handler(index: u8, handler: unsafe extern "C" fn()) {
    register_gate(index, handler, GATE_INTERRUPT_32);
}

/// Install handler with custom flags (e.g. DPL=3 for user-callable syscalls: 0xEE).
pub fn register_gate(index: u8, handler: unsafe extern "C" fn(), flags: u8) {
    unsafe {
        IDT[index as usize] = IdtEntry::from_handler_flags(handler, flags);
    }
    if unsafe { IDT_REGISTERED } {
        load_idt();
    }
}

/// Present, DPL=3, 32-bit interrupt gate — user mode may `int n`.
pub const GATE_INTERRUPT_USER: u8 = 0xEE;


/// How many gates are currently present (flags bit 7).
pub fn present_gate_count() -> usize {
    let mut n = 0;
    unsafe {
        for i in 0..256 {
            // Read flags via raw pointer (no shared ref to static mut).
            let flags = core::ptr::addr_of!(IDT[i].flags).read_unaligned();
            if flags & 0x80 != 0 {
                n += 1;
            }
        }
    }
    n
}

/// Physical/linear base address of the IDT array.
pub fn idt_base() -> u32 {
    addr_of!(IDT) as u32
}

pub fn is_registered() -> bool {
    unsafe { IDT_REGISTERED }
}
