//! Scrolling VGA text console for the shell.
//!
//! Output always goes to the classic text buffer at `0xB8000` (80×25) so
//! qemu-connect / MCP glyph scrape keeps working. Boot can load a custom
//! 8×16 **bitmap font** into the VGA character generator for nicer glyphs
//! without leaving text mode.

use crate::vga::load_vga_bitmap_font;
use crate::x86::io::{inb, outb};

const VGA: *mut u16 = 0xB8000 as *mut u16;
pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;

const VGA_CMD: u16 = 0x3D4;
const VGA_DATA: u16 = 0x3D5;

static mut ROW: usize = 0;
static mut COL: usize = 0;
static mut COLOR: u8 = 0x07;
static mut INVERSE: bool = false;
static mut CURSOR_ENABLED: bool = true;
static mut FONT_LOADED: bool = false;

/// One-time console bring-up: load bitmap font, then clear the text buffer.
pub fn init() {
    unsafe {
        if !FONT_LOADED {
            load_vga_bitmap_font();
            FONT_LOADED = true;
        }
    }
    clear();
}

fn cell(ch: u8, color: u8) -> u16 {
    (color as u16) << 8 | ch as u16
}

fn active_color() -> u8 {
    unsafe {
        if INVERSE {
            // Swap fg/bg nybbles for a solid reverse block.
            let c = COLOR;
            ((c & 0x0F) << 4) | ((c & 0xF0) >> 4)
        } else {
            COLOR
        }
    }
}

pub fn set_color(c: u8) {
    unsafe {
        COLOR = c;
    }
}

pub fn set_inverse(on: bool) {
    unsafe {
        INVERSE = on;
    }
}

/// Enable/disable the blinking hardware cursor.
pub fn set_cursor_enabled(on: bool) {
    unsafe {
        CURSOR_ENABLED = on;
        if on {
            // Scanlines 14–15 → underscore-style cursor (visible on most VGA).
            outb(VGA_CMD, 0x0A);
            let start = inb(VGA_DATA);
            outb(VGA_DATA, (start & 0xC0) | 0x0E);
            outb(VGA_CMD, 0x0B);
            let end = inb(VGA_DATA);
            outb(VGA_DATA, (end & 0xE0) | 0x0F);
            update_hw_cursor();
        } else {
            outb(VGA_CMD, 0x0A);
            let start = inb(VGA_DATA);
            outb(VGA_DATA, start | 0x20);
        }
    }
}

/// Move the VGA hardware cursor to the software row/col.
pub fn update_hw_cursor() {
    unsafe {
        if !CURSOR_ENABLED {
            return;
        }
        let row = ROW.min(HEIGHT - 1);
        let col = COL.min(WIDTH - 1);
        let pos = (row * WIDTH + col) as u16;
        outb(VGA_CMD, 0x0E);
        outb(VGA_DATA, (pos >> 8) as u8);
        outb(VGA_CMD, 0x0F);
        outb(VGA_DATA, (pos & 0xFF) as u8);
    }
}

pub fn clear() {
    unsafe {
        let color = COLOR;
        for i in 0..(WIDTH * HEIGHT) {
            VGA.add(i).write_volatile(cell(b' ', color));
        }
        ROW = 0;
        COL = 0;
        INVERSE = false;
    }
    set_cursor_enabled(true);
    update_hw_cursor();
}

fn scroll() {
    unsafe {
        for r in 1..HEIGHT {
            for c in 0..WIDTH {
                let v = VGA.add(r * WIDTH + c).read_volatile();
                VGA.add((r - 1) * WIDTH + c).write_volatile(v);
            }
        }
        let color = COLOR;
        for c in 0..WIDTH {
            VGA.add((HEIGHT - 1) * WIDTH + c).write_volatile(cell(b' ', color));
        }
        if ROW > 0 {
            ROW = HEIGHT - 1;
        }
    }
}

pub fn put_char(ch: u8) {
    unsafe {
        match ch {
            b'\n' => {
                COL = 0;
                ROW += 1;
                if ROW >= HEIGHT {
                    scroll();
                }
            }
            0x08 => {
                // backspace
                if COL > 0 {
                    COL -= 1;
                    VGA.add(ROW * WIDTH + COL)
                        .write_volatile(cell(b' ', COLOR));
                }
            }
            b'\t' => {
                for _ in 0..4 {
                    put_char(b' ');
                }
                return; // put_char already updates cursor in recursion
            }
            // Printable ASCII + CP437 extended (block cursor 0xDB, etc.)
            c if (0x20..=0xFF).contains(&c) && c != 0x7F => {
                let color = active_color();
                VGA.add(ROW * WIDTH + COL).write_volatile(cell(c, color));
                COL += 1;
                if COL >= WIDTH {
                    COL = 0;
                    ROW += 1;
                    if ROW >= HEIGHT {
                        scroll();
                    }
                }
            }
            _ => {}
        }
    }
    update_hw_cursor();
}

pub fn write_bytes(s: &[u8]) {
    for &b in s {
        put_char(b);
    }
}

pub fn write_str(s: &str) {
    write_bytes(s.as_bytes());
}

pub fn write_u64(mut v: u64) {
    if v == 0 {
        put_char(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 0;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        put_char(buf[i]);
    }
}

pub fn write_hex64(v: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    write_str("0x");
    for n in 0..16 {
        let shift = (15 - n) * 4;
        let nib = ((v >> shift) & 0xF) as usize;
        put_char(HEX[nib]);
    }
}

/// Minimal print! style without alloc: console::print("text")
pub fn print(s: &str) {
    write_str(s);
}

pub fn println(s: &str) {
    write_str(s);
    put_char(b'\n');
}
