//! Unix98 PTY pair (P11b): `/dev/ptmx` master + `/dev/pts/N` slave.
//!
//! Two rings: master write → slave read, slave write → master read.
//! Slave is a tty (termios / winsize / TIOCSCTTY). Master is the mux fd.

use crate::tty;

pub const MAX_PTY: usize = 4;
const RING: usize = 1024;

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
    let dst = &mut ptys()[n].termios;
    let k = src.len().min(dst.len());
    dst[..k].copy_from_slice(&src[..k]);
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

/// Master write → slave stdin.
pub fn master_write(n: usize, data: &[u8]) -> Result<usize, i32> {
    end_write(n, data, true)
}

/// Slave write → master stdout.
pub fn slave_write(n: usize, data: &[u8]) -> Result<usize, i32> {
    end_write(n, data, false)
}

/// Slave read (stdin).
pub fn slave_read(n: usize, buf: &mut [u8]) -> Result<usize, i32> {
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
