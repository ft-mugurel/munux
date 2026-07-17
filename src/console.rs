//! Scrolling VGA text console for the shell.

const VGA: *mut u16 = 0xB8000 as *mut u16;
pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;

static mut ROW: usize = 0;
static mut COL: usize = 0;
static mut COLOR: u8 = 0x07;

fn cell(ch: u8, color: u8) -> u16 {
    (color as u16) << 8 | ch as u16
}

pub fn set_color(c: u8) {
    unsafe {
        COLOR = c;
    }
}

pub fn clear() {
    unsafe {
        for i in 0..(WIDTH * HEIGHT) {
            VGA.add(i).write_volatile(cell(b' ', COLOR));
        }
        ROW = 0;
        COL = 0;
    }
}

fn scroll() {
    unsafe {
        for r in 1..HEIGHT {
            for c in 0..WIDTH {
                let v = VGA.add(r * WIDTH + c).read_volatile();
                VGA.add((r - 1) * WIDTH + c).write_volatile(v);
            }
        }
        for c in 0..WIDTH {
            VGA.add((HEIGHT - 1) * WIDTH + c).write_volatile(cell(b' ', COLOR));
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
            }
            c if c >= 0x20 && c < 0x7F => {
                VGA.add(ROW * WIDTH + COL).write_volatile(cell(c, COLOR));
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
