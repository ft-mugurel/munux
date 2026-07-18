//! Interactive munux shell (line editor + built-in commands).

mod commands;

use crate::console;
use crate::interrupts::keyboard::init::pop_char;

const PROMPT: &str = "munux> ";
const LINE_CAP: usize = 120;

static mut LINE: [u8; LINE_CAP] = [0; LINE_CAP];
static mut LINE_LEN: usize = 0;

pub fn init() {
    console::set_color(0x0F);
    console::println("munux kernel shell (debug). Type `help` — `run sh` re-enters U8 init.");
    console::set_color(0x07);
    print_prompt();
}

fn print_prompt() {
    console::set_color(0x0B);
    console::print(PROMPT);
    console::set_color(0x07);
}

/// Drain keyboard buffer (call from idle loop).
pub fn poll() {
    while let Some(b) = pop_char() {
        on_byte(b);
    }
}

fn on_byte(b: u8) {
    match b {
        b'\n' | b'\r' => submit(),
        0x08 | 0x7F => backspace(),
        b'\t' => {
            for _ in 0..4 {
                push(b' ');
            }
        }
        c if c >= 0x20 && c < 0x7F => push(c),
        _ => {}
    }
}

fn push(b: u8) {
    unsafe {
        if LINE_LEN >= LINE_CAP {
            return;
        }
        LINE[LINE_LEN] = b;
        LINE_LEN += 1;
    }
    console::put_char(b);
}

fn backspace() {
    unsafe {
        if LINE_LEN == 0 {
            return;
        }
        LINE_LEN -= 1;
        LINE[LINE_LEN] = 0;
    }
    console::put_char(0x08);
}

fn submit() {
    console::put_char(b'\n');
    let (buf, len) = unsafe {
        let len = LINE_LEN;
        LINE_LEN = 0;
        let mut tmp = [0u8; LINE_CAP];
        tmp[..len].copy_from_slice(&LINE[..len]);
        (tmp, len)
    };
    let line = core::str::from_utf8(&buf[..len]).unwrap_or("");
    let line = line.trim();
    if !line.is_empty() {
        commands::dispatch(line);
    }
    print_prompt();
}
