//! Minimal VGA text helpers for early bring-up (no full console yet).

const VGA: *mut u16 = 0xB8000 as *mut u16;
const WIDTH: usize = 80;
const HEIGHT: usize = 25;

pub fn clear_screen() {
    unsafe {
        for i in 0..(WIDTH * HEIGHT) {
            VGA.add(i).write_volatile(0x0720);
        }
    }
}

pub fn println_line(row: usize, s: &[u8], color: u8) {
    if row >= HEIGHT {
        return;
    }
    print_str(row, 0, s, color);
}

pub fn print_str(row: usize, col: usize, s: &[u8], color: u8) {
    if row >= HEIGHT {
        return;
    }
    unsafe {
        for (i, &b) in s.iter().enumerate() {
            let c = col + i;
            if c >= WIDTH {
                break;
            }
            let cell = (color as u16) << 8 | b as u16;
            VGA.add(row * WIDTH + c).write_volatile(cell);
        }
    }
}

pub fn print_u64(row: usize, col: usize, mut value: u64, color: u8) {
    let mut buf = [0u8; 20];
    let mut i = 0;
    if value == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while value > 0 && i < 20 {
            buf[i] = b'0' + (value % 10) as u8;
            value /= 10;
            i += 1;
        }
        // reverse
        let mut a = 0;
        let mut b = i - 1;
        while a < b {
            buf.swap(a, b);
            a += 1;
            b -= 1;
        }
    }
    print_str(row, col, &buf[..i], color);
}

pub fn print_hex64(row: usize, col: usize, value: u64, color: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [0u8; 18];
    buf[0] = b'0';
    buf[1] = b'x';
    for n in 0..16 {
        let shift = (15 - n) * 4;
        let nib = ((value >> shift) & 0xF) as usize;
        buf[2 + n] = HEX[nib];
    }
    print_str(row, col, &buf, color);
}
