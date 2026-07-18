//! PS/2 keyboard IRQ1 → vector 33 (ring buffer for shell).

use crate::interrupts::idt::register_interrupt_handler;
use crate::interrupts::keyboard::character_map::keycode_to_char;
use crate::interrupts::keyboard::keycode::{decode_set1_scancode, KeyCode, KeyEvent, Modifiers};
use crate::interrupts::pic;
use crate::x86::io::{inb, outw};

static mut EXTENDED_SCANCODE: bool = false;
static mut MODIFIERS: Modifiers = Modifiers::empty();

const SCANCODE_EXTENDED_PREFIX: u8 = 0xE0;
const KEYBOARD_DATA_PORT: u16 = 0x60;
const KEYBOARD_IRQ_VECTOR: u8 = 33;

const BUF_CAP: usize = 128;
static mut KBUF: [u8; BUF_CAP] = [0; BUF_CAP];
static mut KHEAD: usize = 0;
static mut KTAIL: usize = 0;
static mut KLEN: usize = 0;

fn buf_push(b: u8) {
    unsafe {
        if KLEN >= BUF_CAP {
            KHEAD = (KHEAD + 1) % BUF_CAP;
            KLEN -= 1;
        }
        KBUF[KTAIL] = b;
        KTAIL = (KTAIL + 1) % BUF_CAP;
        KLEN += 1;
    }
}

pub fn pop_char() -> Option<u8> {
    unsafe {
        if KLEN == 0 {
            return None;
        }
        let b = KBUF[KHEAD];
        KHEAD = (KHEAD + 1) % BUF_CAP;
        KLEN -= 1;
        Some(b)
    }
}

pub fn buffered_len() -> usize {
    unsafe { KLEN }
}

/// Push a decoded byte as if typed (tests / automated smoke).
pub fn inject_char(b: u8) {
    buf_push(b);
}

/// Inject a short string (e.g. for U2 smoke without QEMU sendkey races).
pub fn inject_str(s: &[u8]) {
    for &b in s {
        buf_push(b);
    }
}

fn handle_key_press(event: KeyEvent, modifiers: Modifiers) -> bool {
    if event.key == KeyCode::Delete && modifiers.ctrl() && modifiers.alt() {
        return true;
    }
    match event.key {
        KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight => {}
        KeyCode::F1 | KeyCode::F2 | KeyCode::F3 | KeyCode::F4 | KeyCode::F5 | KeyCode::F6 => {}
        _ => {
            if !modifiers.has_text_blocking_modifier() {
                if let Some(ch) = keycode_to_char(event.key, modifiers) {
                    if ch == '\n' || ch == '\r' {
                        buf_push(b'\n');
                    } else if ch == '\x08' {
                        buf_push(0x08);
                    } else if (ch as u32) >= 0x20 && (ch as u32) < 0x7F {
                        buf_push(ch as u8);
                    }
                }
            }
        }
    }
    false
}

fn shutdown_system() -> ! {
    unsafe {
        outw(0x604, 0x2000);
        outw(0xB004, 0x2000);
        outw(0x4004, 0x3400);
    }
    loop {
        unsafe {
            core::arch::asm!("cli; hlt");
        }
    }
}

#[no_mangle]
pub extern "C" fn keyboard_interrupt_handler() {
    let mut should_shutdown = false;
    let scancode = unsafe { inb(KEYBOARD_DATA_PORT) };

    unsafe {
        if scancode == SCANCODE_EXTENDED_PREFIX {
            EXTENDED_SCANCODE = true;
        } else {
            if let Some(event) = decode_set1_scancode(scancode, EXTENDED_SCANCODE) {
                let mut modifiers = MODIFIERS;
                modifiers.update_for_event(event);
                MODIFIERS = modifiers;
                if event.pressed {
                    should_shutdown = handle_key_press(event, modifiers);
                }
            }
            EXTENDED_SCANCODE = false;
        }
    }

    unsafe {
        pic::eoi_master();
    }

    if should_shutdown {
        shutdown_system();
    }
}

extern "C" {
    fn isr_keyboard();
}

pub fn init_keyboard() {
    register_interrupt_handler(KEYBOARD_IRQ_VECTOR, isr_keyboard);
}
