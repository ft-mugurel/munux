//! PS/2 keyboard IRQ1 → vector 33 (ring buffer for shell).
//!
//! IRQ handler and the main/syscall path share this buffer. All index/len
//! accesses use volatile so a blocking `hlt` loop re-reads after wake-up
//! (plain loads + `asm!(..., options(nomem))` can be optimized into an
//! infinite sleep — that broke `run sh` / userland `read`).

use core::ptr::{addr_of, addr_of_mut};
use core::sync::atomic::{compiler_fence, Ordering};

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

#[inline]
fn klen() -> usize {
    unsafe { core::ptr::read_volatile(addr_of!(KLEN)) }
}

#[inline]
fn set_klen(v: usize) {
    unsafe {
        core::ptr::write_volatile(addr_of_mut!(KLEN), v);
    }
}

/// Push one byte. Caller must ensure mutual exclusion (IRQ with IF=0, or cli).
fn buf_push_unlocked(b: u8) {
    unsafe {
        let mut len = klen();
        let mut head = core::ptr::read_volatile(addr_of!(KHEAD));
        let mut tail = core::ptr::read_volatile(addr_of!(KTAIL));
        if len >= BUF_CAP {
            head = (head + 1) % BUF_CAP;
            len -= 1;
        }
        core::ptr::write_volatile(addr_of_mut!(KBUF[tail]), b);
        tail = (tail + 1) % BUF_CAP;
        core::ptr::write_volatile(addr_of_mut!(KHEAD), head);
        core::ptr::write_volatile(addr_of_mut!(KTAIL), tail);
        set_klen(len + 1);
    }
}

/// IRQ path: interrupt gate already has IF=0 — do not sti here.
fn buf_push_from_irq(b: u8) {
    buf_push_unlocked(b);
}

/// Process context: mask IRQ1 races with cli/sti.
fn buf_push_from_process(b: u8) {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        buf_push_unlocked(b);
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

pub fn pop_char() -> Option<u8> {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        let len = klen();
        if len == 0 {
            core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
            return None;
        }
        let head = core::ptr::read_volatile(addr_of!(KHEAD));
        let b = core::ptr::read_volatile(addr_of!(KBUF[head]));
        core::ptr::write_volatile(addr_of_mut!(KHEAD), (head + 1) % BUF_CAP);
        set_klen(len - 1);
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
        Some(b)
    }
}

pub fn buffered_len() -> usize {
    klen()
}

/// Push a decoded byte as if typed (tests / automated smoke).
pub fn inject_char(b: u8) {
    buf_push_from_process(b);
}

/// Inject a short string (e.g. for U2 smoke without QEMU sendkey races).
pub fn inject_str(s: &[u8]) {
    for &b in s {
        buf_push_from_process(b);
    }
}

/// Block until at least one byte is available (IRQ-safe).
///
/// Must re-check the buffer after every wake: do not use `options(nomem)` on
/// the `hlt` or LLVM may hoist the empty-buffer check out of the loop.
pub fn wait_for_input() {
    loop {
        if klen() > 0 {
            compiler_fence(Ordering::SeqCst);
            return;
        }
        unsafe {
            // Memory may change via IRQ handlers while halted — no `nomem`.
            core::arch::asm!("sti; hlt", options(nostack));
        }
        compiler_fence(Ordering::SeqCst);
    }
}

fn handle_key_press(event: KeyEvent, modifiers: Modifiers) -> bool {
    if event.key == KeyCode::Delete && modifiers.ctrl() && modifiers.alt() {
        return true;
    }
    match event.key {
        // Delete key → DEL (shells treat like backspace). Ctrl+Alt+Del still above.
        KeyCode::Delete if !modifiers.ctrl() && !modifiers.alt() => {
            buf_push_from_irq(0x7F);
        }
        KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight => {}
        KeyCode::F1 | KeyCode::F2 | KeyCode::F3 | KeyCode::F4 | KeyCode::F5 | KeyCode::F6 => {}
        _ => {
            if !modifiers.has_text_blocking_modifier() {
                if let Some(ch) = keycode_to_char(event.key, modifiers) {
                    if ch == '\n' || ch == '\r' {
                        buf_push_from_irq(b'\n');
                    } else if ch == '\x08' {
                        buf_push_from_irq(0x08);
                    } else if (ch as u32) >= 0x20 && (ch as u32) < 0x7F {
                        buf_push_from_irq(ch as u8);
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
