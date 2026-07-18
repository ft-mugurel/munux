//! 16550 UART (COM1 @ 0x3F8) — serial console for typing in the QEMU terminal.
//!
//! With `qemu -serial stdio`, keys in the host terminal reach the guest even
//! when the VGA window is unfocused. VGA output is mirrored to serial too.

use crate::x86::io::{inb, outb};

const COM1: u16 = 0x3F8;

static mut READY: bool = false;

/// Init COM1: 115200 8N1, FIFO on, IRQs off (we poll).
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // disable UART IRQs
        outb(COM1 + 3, 0x80); // DLAB on
        outb(COM1 + 0, 0x01); // divisor lo — 115200
        outb(COM1 + 1, 0x00); // divisor hi
        outb(COM1 + 3, 0x03); // 8N1, DLAB off
        outb(COM1 + 2, 0xC7); // FIFO enable, clear
        outb(COM1 + 4, 0x0F); // OUT2/RTS/DTR (normal)
        READY = true;
    }
}

pub fn is_ready() -> bool {
    unsafe { READY }
}

fn thr_empty() -> bool {
    unsafe { inb(COM1 + 5) & 0x20 != 0 }
}

fn data_ready() -> bool {
    unsafe { inb(COM1 + 5) & 0x01 != 0 }
}

/// Write one byte (waits briefly for THR).
pub fn write_byte(b: u8) {
    if !is_ready() {
        return;
    }
    let mut spins = 0u32;
    while !thr_empty() {
        spins = spins.wrapping_add(1);
        if spins > 100_000 {
            break;
        }
        core::hint::spin_loop();
    }
    unsafe {
        outb(COM1, b);
    }
}

pub fn write_bytes(data: &[u8]) {
    for &b in data {
        if b == b'\n' {
            write_byte(b'\r');
        }
        write_byte(b);
    }
}

/// Non-blocking read of one byte if available.
pub fn try_read_byte() -> Option<u8> {
    if !is_ready() || !data_ready() {
        return None;
    }
    let b = unsafe { inb(COM1) };
    // CR → NL (Enter from most terminals)
    if b == b'\r' {
        Some(b'\n')
    } else {
        Some(b)
    }
}

/// Drain UART RX into the keyboard ring buffer.
pub fn poll_to_keyboard() {
    use crate::interrupts::keyboard::init::inject_char;
    while let Some(b) = try_read_byte() {
        if b == b'\n' || b == b'\t' || b == 0x08 || b == 0x7F {
            inject_char(if b == 0x7F { 0x08 } else { b });
        } else if (0x20..0x7F).contains(&b) {
            inject_char(b);
        }
    }
}
