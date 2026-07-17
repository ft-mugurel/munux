//! 8259 PIC remapping for x86_64.
//!
//! Master IRQs 0–7  → IDT vectors 32–39
//! Slave  IRQs 8–15 → IDT vectors 40–47

use crate::x86::io::outb;

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const ICW1_INIT: u8 = 0x11;
const ICW4_8086: u8 = 0x01;

/// Remap PIC and unmask IRQ0 (timer) + IRQ1 (keyboard).
pub unsafe fn init_pic() {
    // Start init sequence (cascade mode)
    outb(PIC1_CMD, ICW1_INIT);
    outb(PIC2_CMD, ICW1_INIT);

    // ICW2: vector offsets
    outb(PIC1_DATA, 0x20); // master → 32
    outb(PIC2_DATA, 0x28); // slave  → 40

    // ICW3: cascade identity
    outb(PIC1_DATA, 0x04); // slave on IRQ2
    outb(PIC2_DATA, 0x02); // cascade identity 2

    // ICW4: 8086 mode
    outb(PIC1_DATA, ICW4_8086);
    outb(PIC2_DATA, ICW4_8086);

    // Mask all, then unmask timer+keyboard on master
    outb(PIC1_DATA, 0xFF);
    outb(PIC2_DATA, 0xFF);
    outb(PIC1_DATA, 0xFC); // bits 0,1 clear → IRQ0 + IRQ1
    outb(PIC2_DATA, 0xFF);
}

/// End-of-interrupt to master PIC.
pub unsafe fn eoi_master() {
    outb(PIC1_CMD, 0x20);
}
