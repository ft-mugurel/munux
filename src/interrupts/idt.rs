//! Interrupt Descriptor Table (64-bit gates).

use core::arch::asm;
use core::ptr::addr_of;

use crate::gdt::gdt::KERNEL_CODE_SELECTOR;

/// 64-bit interrupt gate: P=1, DPL=0, type=0xE.
const GATE_INTERRUPT_64: u8 = 0x8E;

/// One IDT entry (16 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn empty() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    fn from_handler(handler: unsafe extern "C" fn(), ist: u8) -> Self {
        let addr = handler as usize as u64;
        Self {
            offset_low: addr as u16,
            selector: KERNEL_CODE_SELECTOR,
            ist: ist & 0x7,
            type_attr: GATE_INTERRUPT_64,
            offset_mid: (addr >> 16) as u16,
            offset_high: (addr >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::empty(); 256];

pub fn init_idt() {
    unsafe {
        for i in 0..256 {
            IDT[i] = IdtEntry::empty();
        }
    }
    load_idt();
}

pub fn load_idt() {
    let ptr = IdtPointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: addr_of!(IDT) as u64,
    };
    unsafe {
        asm!(
            "lidt [{}]",
            in(reg) &ptr,
            options(readonly, nostack, preserves_flags)
        );
    }
}

pub fn register_interrupt_handler(index: u8, handler: unsafe extern "C" fn()) {
    register_gate(index, handler, 0);
}

/// `ist` = 0 (current stack) or 1–7 for TSS IST.
pub fn register_gate(index: u8, handler: unsafe extern "C" fn(), ist: u8) {
    unsafe {
        IDT[index as usize] = IdtEntry::from_handler(handler, ist);
    }
    load_idt();
}

pub fn present_gate_count() -> usize {
    let mut n = 0;
    unsafe {
        for i in 0..256 {
            let attr = core::ptr::addr_of!(IDT[i].type_attr).read_unaligned();
            if attr & 0x80 != 0 {
                n += 1;
            }
        }
    }
    n
}

pub fn idt_base() -> u64 {
    addr_of!(IDT) as u64
}
