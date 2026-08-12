//! Unix98 PTY pair (P11b) + n_tty cook (P11c).
//!
//! Master write is cooked (`ICANON` / `ECHO` / `ISIG`) into the slave ring.
//! Slave write may apply `OPOST`/`ONLCR`. Slave is a tty (termios / TIOCSCTTY).

use crate::tty;

pub const MAX_PTY: usize = 4;
const RING: usize = 1024;
const LINE_CAP: usize = 128;

/// Linux: `ctty == 2 + n` means PTY n (0 = none, 1 = console).
pub fn ctty_for(n: usize) -> i32 {
    2 + n as i32
}

pub fn index_from_ctty(ctty: i32) -> Option<usize> {
    if ctty >= 2 {
        let n = (ctty - 2) as usize;
        if n < MAX_PTY {
            return Some(n);
        }
    }
    None
}

struct Ring {
    data: [u8; RING],
    r: u16,
    len: u16,
}

impl Ring {
    const fn empty() -> Self {
        Self {
            data: [0; RING],
            r: 0,
            len: 0,
        }
    }

    fn pop(&mut self, out: &mut [u8]) -> usize {
        let n = (self.len as usize).min(out.len());
        for i in 0..n {
            out[i] = self.data[(self.r as usize + i) % RING];
        }
        self.r = ((self.r as usize + n) % RING) as u16;
        self.len -= n as u16;
        n
    }

    fn push(&mut self, src: &[u8]) -> usize {
        let space = RING - self.len as usize;
        let n = space.min(src.len());
        let w = (self.r as usize + self.len as usize) % RING;
        for i in 0..n {
            self.data[(w + i) % RING] = src[i];
        }
        self.len += n as u16;
        n
    }
}

struct Pty {
    used: bool,
    locked: bool,
    nmaster: u8,
    nslave: u8,
    to_slave: Ring,
    to_master: Ring,
    line: [u8; LINE_CAP],
    line_len: u8,
    termios: [u8; tty::TERMIOS_LEN],
    rows: u16,
    cols: u16,
    fg_pgid: i32,
    sid: i32,
}

impl Pty {
    const fn empty() -> Self {
        Self {
            used: false,
            locked: true,
            nmaster: 0,
            nslave: 0,
            to_slave: Ring::empty(),
            to_master: Ring::empty(),
            line: [0; LINE_CAP],
            line_len: 0,
            termios: [0; tty::TERMIOS_LEN],
            rows: 24,
            cols: 80,
            fg_pgid: 0,
            sid: 0,
        }
    }
}

static mut PTYS: [Pty; MAX_PTY] = [
    Pty::empty(),
    Pty::empty(),
    Pty::empty(),
    Pty::empty(),
];

fn ptys() -> &'static mut [Pty; MAX_PTY] {
    unsafe { &mut *core::ptr::addr_of_mut!(PTYS) }
}

fn schedule_peer() {
    crate::syscalls::try_run_ready_child();
}

/// Allocate a new pair; caller holds the master. Slave starts locked.
pub fn open_master() -> Result<usize, ()> {
    let p = ptys();
    for (i, slot) in p.iter_mut().enumerate() {
        if !slot.used {
            *slot = Pty::empty();
            slot.used = true;
            slot.locked = true;
            slot.nmaster = 1;
            slot.nslave = 0;
            tty::fill_default_termios(&mut slot.termios);
            return Ok(i);
        }
    }
    Err(())
}

pub fn open_slave(n: usize) -> Result<usize, i32> {
    if n >= MAX_PTY {
        return Err(-2); // ENOENT
    }
    let p = &mut ptys()[n];
    if !p.used {
        return Err(-2);
    }
    if p.locked {
        return Err(-5); // EIO
    }
    p.nslave = p.nslave.saturating_add(1);
    Ok(n)
}

pub fn dup_master(n: usize) {
    if n < MAX_PTY && ptys()[n].used {
        ptys()[n].nmaster = ptys()[n].nmaster.saturating_add(1);
    }
}

pub fn dup_slave(n: usize) {
    if n < MAX_PTY && ptys()[n].used {
        ptys()[n].nslave = ptys()[n].nslave.saturating_add(1);
    }
}

pub fn close_master(n: usize) {
    if n >= MAX_PTY {
        return;
    }
    let p = &mut ptys()[n];
    if !p.used {
        return;
    }
    if p.nmaster > 0 {
        p.nmaster -= 1;
    }
    maybe_free(n);
}

pub fn close_slave(n: usize) {
    if n >= MAX_PTY {
        return;
    }
    let p = &mut ptys()[n];
    if !p.used {
        return;
    }
    if p.nslave > 0 {
        p.nslave -= 1;
    }
    maybe_free(n);
}

fn maybe_free(n: usize) {
    let p = &mut ptys()[n];
    if p.nmaster == 0 && p.nslave == 0 {
        *p = Pty::empty();
    }
}

pub fn is_used(n: usize) -> bool {
    n < MAX_PTY && ptys()[n].used
}

pub fn is_locked(n: usize) -> bool {
    n < MAX_PTY && ptys()[n].used && ptys()[n].locked
}

pub fn set_locked(n: usize, locked: bool) -> bool {
    if n >= MAX_PTY || !ptys()[n].used {
        return false;
    }
    ptys()[n].locked = locked;
    true
}

pub fn get_termios(n: usize, out: &mut [u8]) {
    if n >= MAX_PTY || !ptys()[n].used {
        return;
    }
    let src = &ptys()[n].termios;
    let k = out.len().min(src.len());
    out[..k].copy_from_slice(&src[..k]);
}

pub fn set_termios(n: usize, src: &[u8]) {
    if n >= MAX_PTY || !ptys()[n].used {
        return;
    }
    let old_l = tty::termios_u32(&ptys()[n].termios, 12);
    let dst = &mut ptys()[n].termios;
    let k = src.len().min(dst.len());
    dst[..k].copy_from_slice(&src[..k]);
    let new_l = tty::termios_u32(&ptys()[n].termios, 12);
    // Leaving canonical mode: pending line becomes readable (Linux-ish).
    if (old_l & tty::ICANON) != 0 && (new_l & tty::ICANON) == 0 {
        let len = ptys()[n].line_len as usize;
        if len > 0 {
            let mut tmp = [0u8; LINE_CAP];
            tmp[..len].copy_from_slice(&ptys()[n].line[..len]);
            ptys()[n].line_len = 0;
            let _ = push_slave(n, &tmp[..len]);
        }
    }
}

pub fn winsize(n: usize) -> (u16, u16) {
    if n >= MAX_PTY || !ptys()[n].used {
        return (24, 80);
    }
    (ptys()[n].rows, ptys()[n].cols)
}

pub fn set_winsize(n: usize, rows: u16, cols: u16) {
    if n >= MAX_PTY || !ptys()[n].used {
        return;
    }
    if rows > 0 {
        ptys()[n].rows = rows;
    }
    if cols > 0 {
        ptys()[n].cols = cols;
    }
}

pub fn fg_pgid(n: usize) -> i32 {
    if n >= MAX_PTY || !ptys()[n].used {
        return 0;
    }
    ptys()[n].fg_pgid
}

pub fn set_fg_pgid(n: usize, pgid: i32) {
    if n < MAX_PTY && ptys()[n].used && pgid > 0 {
        ptys()[n].fg_pgid = pgid;
    }
}

pub fn sid(n: usize) -> i32 {
    if n >= MAX_PTY || !ptys()[n].used {
        return 0;
    }
    ptys()[n].sid
}

pub fn set_sid(n: usize, sid: i32) {
    if n < MAX_PTY && ptys()[n].used {
        ptys()[n].sid = sid;
    }
}

/// Master write → n_tty → slave stdin (and echo → master).
pub fn master_write(n: usize, data: &[u8]) -> Result<usize, i32> {
    if n >= MAX_PTY {
        return Err(-22);
    }
    if data.is_empty() {
        return Ok(0);
    }
    if !ptys()[n].used {
        return Err(-9);
    }
    for &b in data {
        input_byte(n, b);
    }
    Ok(data.len())
}

/// Slave write → master stdout (`OPOST`/`ONLCR` if set).
pub fn slave_write(n: usize, data: &[u8]) -> Result<usize, i32> {
    if n >= MAX_PTY {
        return Err(-22);
    }
    if data.is_empty() {
        return Ok(0);
    }
    let lflag = {
        let p = &ptys()[n];
        if !p.used {
            return Err(-9);
        }
        tty::termios_u32(&p.termios, 12)
    };
    let rc = crate::tty::job_check_write(ctty_for(n), fg_pgid(n), (lflag & tty::TOSTOP) != 0);
    if rc < 0 {
        return Err(rc);
    }
    let oflag = {
        let p = &ptys()[n];
        if !p.used {
            return Err(-9);
        }
        tty::termios_u32(&p.termios, 4)
    };
    let cook = (oflag & tty::OPOST) != 0 && (oflag & tty::ONLCR) != 0;
    if !cook {
        return end_write(n, data, false);
    }
    let mut tmp = [0u8; 256];
    let mut o = 0usize;
    for &b in data {
        if o + 2 > tmp.len() {
            let _ = end_write(n, &tmp[..o], false);
            o = 0;
        }
        if b == b'\n' {
            tmp[o] = b'\r';
            o += 1;
        }
        tmp[o] = b;
        o += 1;
    }
    if o > 0 {
        end_write(n, &tmp[..o], false)?;
    }
    Ok(data.len())
}

fn input_byte(n: usize, raw: u8) {
    let (iflag, lflag) = {
        let p = &ptys()[n];
        (
            tty::termios_u32(&p.termios, 0),
            tty::termios_u32(&p.termios, 12),
        )
    };
    let vintr = tty::termios_cc(&ptys()[n].termios, tty::VINTR);
    let vquit = tty::termios_cc(&ptys()[n].termios, tty::VQUIT);
    let verase = tty::termios_cc(&ptys()[n].termios, tty::VERASE);
    let vkill = tty::termios_cc(&ptys()[n].termios, tty::VKILL);
    let veof = tty::termios_cc(&ptys()[n].termios, tty::VEOF);

    let mut b = raw;
    if (iflag & tty::ICRNL) != 0 && b == b'\r' {
        b = b'\n';
    }

    if (lflag & tty::ISIG) != 0 && b != 0 && (b == vintr || b == vquit) {
        let pg = ptys()[n].fg_pgid;
        let sig = if b == vquit {
            crate::process::signal_queue::SIGQUIT
        } else {
            crate::process::signal_queue::SIGINT
        };
        if pg > 0 {
            let me_pg = crate::process::with_current(|p| p.pgid).unwrap_or(0);
            if me_pg == pg {
                crate::syscalls::fatal_signal_exit(sig);
            }
            let _ = crate::process::signal_queue::proc_kill_pgrp(pg, sig);
        }
        return;
    }

    if (lflag & tty::ICANON) == 0 {
        let _ = push_slave(n, &[b]);
        if (lflag & tty::ECHO) != 0 {
            echo_byte(n, b);
        }
        return;
    }

    if b == verase || b == 0x08 {
        erase_one(n, lflag);
        return;
    }
    if b == vkill {
        ptys()[n].line_len = 0;
        if (lflag & tty::ECHOK) != 0 || (lflag & tty::ECHO) != 0 {
            echo_byte(n, b'\n');
        }
        return;
    }
    if b == veof {
        let len = ptys()[n].line_len as usize;
        if len > 0 {
            let mut tmp = [0u8; LINE_CAP];
            tmp[..len].copy_from_slice(&ptys()[n].line[..len]);
            ptys()[n].line_len = 0;
            let _ = push_slave(n, &tmp[..len]);
        }
        return;
    }
    if b == b'\n' {
        let len = ptys()[n].line_len as usize;
        let mut tmp = [0u8; LINE_CAP + 1];
        tmp[..len].copy_from_slice(&ptys()[n].line[..len]);
        tmp[len] = b'\n';
        ptys()[n].line_len = 0;
        let _ = push_slave(n, &tmp[..len + 1]);
        if (lflag & tty::ECHO) != 0 {
            echo_byte(n, b'\n');
        }
        return;
    }

    let p = &mut ptys()[n];
    if (p.line_len as usize) < LINE_CAP {
        p.line[p.line_len as usize] = b;
        p.line_len += 1;
        if (lflag & tty::ECHO) != 0 {
            echo_byte(n, b);
        }
    }
}

fn erase_one(n: usize, lflag: u32) {
    let p = &mut ptys()[n];
    if p.line_len == 0 {
        return;
    }
    p.line_len -= 1;
    if (lflag & tty::ECHO) == 0 {
        return;
    }
    if (lflag & tty::ECHOE) != 0 {
        let _ = push_master(n, b"\x08 \x08");
    } else {
        echo_byte(n, 0x08);
    }
}

fn echo_byte(n: usize, b: u8) {
    let _ = push_master(n, &[b]);
}

fn push_slave(n: usize, data: &[u8]) -> usize {
    if n >= MAX_PTY || !ptys()[n].used {
        return 0;
    }
    ptys()[n].to_slave.push(data)
}

fn push_master(n: usize, data: &[u8]) -> usize {
    if n >= MAX_PTY || !ptys()[n].used {
        return 0;
    }
    ptys()[n].to_master.push(data)
}

/// Slave read (stdin).
pub fn slave_read(n: usize, buf: &mut [u8]) -> Result<usize, i32> {
    let fg = fg_pgid(n);
    let rc = crate::tty::job_check_read(ctty_for(n), fg);
    if rc < 0 {
        return Err(rc);
    }
    end_read(n, buf, true)
}

/// Master read (stdout).
pub fn master_read(n: usize, buf: &mut [u8]) -> Result<usize, i32> {
    end_read(n, buf, false)
}

fn end_write(n: usize, data: &[u8], from_master: bool) -> Result<usize, i32> {
    if n >= MAX_PTY {
        return Err(-22);
    }
    if data.is_empty() {
        return Ok(0);
    }
    let mut written = 0usize;
    for _ in 0..2_000_000 {
        {
            let p = &mut ptys()[n];
            if !p.used {
                return Err(-9);
            }
            let peer = if from_master { p.nslave } else { p.nmaster };
            if peer == 0 && written > 0 {
                return Ok(written);
            }
            // Allow the first write before the slave is opened (parent setup).
            let ring = if from_master {
                &mut p.to_slave
            } else {
                &mut p.to_master
            };
            let npush = ring.push(&data[written..]);
            if npush > 0 {
                written += npush;
                if written == data.len() {
                    return Ok(written);
                }
                continue;
            }
        }
        schedule_peer();
    }
    if written > 0 {
        Ok(written)
    } else {
        Err(-11)
    }
}

fn end_read(n: usize, buf: &mut [u8], slave_side: bool) -> Result<usize, i32> {
    if n >= MAX_PTY || buf.is_empty() {
        return Err(-22);
    }
    for _ in 0..2_000_000 {
        {
            let p = &mut ptys()[n];
            if !p.used {
                return Err(-9);
            }
            let ring = if slave_side {
                &mut p.to_slave
            } else {
                &mut p.to_master
            };
            if ring.len > 0 {
                return Ok(ring.pop(buf));
            }
            let peer = if slave_side { p.nmaster } else { p.nslave };
            if peer == 0 {
                return Ok(0);
            }
        }
        schedule_peer();
    }
    Err(-11)
}

/// Parse `"0"` … `"9"` (single or multi digit, no junk).
pub fn parse_index(name: &str) -> Option<usize> {
    if name.is_empty() || name.len() > 2 {
        return None;
    }
    let mut v = 0usize;
    for b in name.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (b - b'0') as usize;
    }
    if v < MAX_PTY {
        Some(v)
    } else {
        None
    }
}

/// Digit name for getdents (`"0"` …).
pub fn name_of(n: usize, out: &mut [u8; 4]) -> usize {
    if n >= 10 {
        out[0] = b'0' + (n / 10) as u8;
        out[1] = b'0' + (n % 10) as u8;
        2
    } else {
        out[0] = b'0' + n as u8;
        1
    }
}
