//! VGA text output helpers and `print!` / `println!` macros.

use core::fmt::{self, Write};

use super::out;

/// Writer that sends formatted text to the active virtual screen.
pub struct VgaWriter;

impl Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_str(s);
        Ok(())
    }
}

/// Format to VGA (like `print!`).
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = write!($crate::vga::text_mod::print::VgaWriter, $($arg)*);
    }};
}

/// Format to VGA with a trailing newline.
#[macro_export]
macro_rules! println {
    () => {{
        $crate::print!("\n");
    }};
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = writeln!($crate::vga::text_mod::print::VgaWriter, $($arg)*);
    }};
}

fn blank_cell() -> u16 {
    unsafe { (b' ' as u16) | ((out::CURRENT_COLOR.0 as u16) << 8) }
}

fn newline_with_scroll() {
    let screen_index = out::current_screen_index();
    unsafe {
        let cursor = &mut out::SCREEN_CURSORS[screen_index];
        cursor.x = 0;

        let next_line = usize::from(cursor.y) + 1;
        if next_line >= out::SCROLLBACK_LINES {
            out::shift_buffer_up(screen_index);
            cursor.y = (out::SCROLLBACK_LINES - 1) as u16;
        } else {
            cursor.y = next_line as u16;
        }

        let cursor_line = usize::from(cursor.y);
        if cursor_line + 1 > out::SCREEN_USED_LINES[screen_index] {
            out::SCREEN_USED_LINES[screen_index] = cursor_line + 1;
            out::clear_buffer_line(screen_index, cursor_line);
        }

        out::sync_screen_state(screen_index);
    }

    out::render_screen(screen_index);
}

fn backspace() {
    let screen_index = out::current_screen_index();
    unsafe {
        let cursor = &mut out::SCREEN_CURSORS[screen_index];
        if cursor.x > 0 {
            cursor.x -= 1;
        } else if cursor.y > 0 {
            cursor.y -= 1;
            cursor.x = (out::VGA_WIDTH - 1) as u16;
        } else {
            return;
        }

        let index = out::cell_index(usize::from(cursor.y), usize::from(cursor.x));
        out::SCREEN_BUFFERS[screen_index][index] = blank_cell();
        out::sync_screen_state(screen_index);
    }

    out::render_screen(screen_index);
}

fn write_byte(byte: u8) {
    let screen_index = out::current_screen_index();

    match byte {
        b'\n' => newline_with_scroll(),
        b'\r' => unsafe {
            out::SCREEN_CURSORS[screen_index].x = 0;
            out::render_screen(screen_index);
        },
        0x08 => backspace(),
        b'\t' => {
            for _ in 0..4 {
                write_byte(b' ');
            }
        }
        byte => {
            let cursor = out::current_cursor();
            let vga_char = (byte as u16) | ((unsafe { out::CURRENT_COLOR.0 } as u16) << 8);
            out::write_cell(
                screen_index,
                usize::from(cursor.y),
                usize::from(cursor.x),
                vga_char,
            );

            unsafe {
                out::SCREEN_CURSORS[screen_index].x =
                    out::SCREEN_CURSORS[screen_index].x.saturating_add(1);
                if usize::from(out::SCREEN_CURSORS[screen_index].x) >= out::VGA_WIDTH {
                    newline_with_scroll();
                } else {
                    out::sync_screen_state(screen_index);
                    out::render_screen(screen_index);
                }
            }
        }
    }
}

pub fn write_str(text: &str) {
    for &byte in text.as_bytes() {
        write_byte(byte);
    }
}

pub fn write_char(c: char) {
    let byte = if (c as u32) <= 0xFF { c as u8 } else { b'?' };
    write_byte(byte);
}

pub fn newline() {
    newline_with_scroll();
}
