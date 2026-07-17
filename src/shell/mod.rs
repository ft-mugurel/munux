//! Interactive kernel shell for debugging and inspection.

mod commands;

const PROMPT: &str = "kfs> ";
const LINE_CAP: usize = 120;

static mut LINE: [u8; LINE_CAP] = [0; LINE_CAP];
static mut LINE_LEN: usize = 0;

/// Banner + first prompt. Call once after VGA is ready.
pub fn init() {
    crate::println!("KFS shell ready. Type `help` for commands.");
    crate::println!("F1-F6: virtual screens | Shift+Arrows: scrollback");
    crate::println!();
    print_prompt();
}

fn print_prompt() {
    crate::print!("{}", PROMPT);
}

/// Feed one decoded character from the keyboard (echo + line edit).
pub fn on_char(c: char) {
    match c {
        '\n' | '\r' => submit_line(),
        '\x08' => backspace(),
        '\t' => {
            // expand tab as spaces in the line buffer
            for _ in 0..4 {
                push_byte(b' ');
            }
        }
        c if (c as u32) >= 0x20 && (c as u32) < 0x7F => {
            push_byte(c as u8);
        }
        _ => {}
    }
}

fn push_byte(b: u8) {
    unsafe {
        if LINE_LEN >= LINE_CAP {
            return;
        }
        LINE[LINE_LEN] = b;
        LINE_LEN += 1;
    }
    crate::print!("{}", b as char);
}

fn backspace() {
    unsafe {
        if LINE_LEN == 0 {
            return;
        }
        LINE_LEN -= 1;
        LINE[LINE_LEN] = 0;
    }
    // erase last echoed character on screen
    crate::print!("\x08");
}

fn submit_line() {
    crate::println!();

    let (cmd_buf, len) = unsafe {
        let len = LINE_LEN;
        LINE_LEN = 0;
        // copy out so commands can print freely without aliasing LINE
        let mut tmp = [0u8; LINE_CAP];
        tmp[..len].copy_from_slice(&LINE[..len]);
        (tmp, len)
    };

    let line = core::str::from_utf8(&cmd_buf[..len]).unwrap_or("");
    let line = line.trim();

    if !line.is_empty() {
        commands::dispatch(line);
    }

    print_prompt();
}
