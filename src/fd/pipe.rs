//! Cooperative pipes (Phase 7d).
//!
//! Fixed pool of ring buffers. Read blocks (schedules Ready peers) while empty
//! and writers remain; write blocks while full and readers remain.

const MAX_PIPES: usize = 16;
const PIPE_CAP: usize = 4096;

struct Pipe {
    used: bool,
    data: [u8; PIPE_CAP],
    /// Read index into ring.
    r: u16,
    /// Bytes currently stored.
    len: u16,
    nreaders: u8,
    nwriters: u8,
}

impl Pipe {
    const fn empty() -> Self {
        Self {
            used: false,
            data: [0; PIPE_CAP],
            r: 0,
            len: 0,
            nreaders: 0,
            nwriters: 0,
        }
    }
}

static mut PIPES: [Pipe; MAX_PIPES] = [
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
    Pipe::empty(),
];

fn pipes_mut() -> &'static mut [Pipe; MAX_PIPES] {
    unsafe { &mut *core::ptr::addr_of_mut!(PIPES) }
}

/// Allocate a pipe; returns (read_id, write_id) as the same pool index
/// (both ends share the buffer; distinguished by fops).
pub fn alloc() -> Result<usize, ()> {
    let p = pipes_mut();
    for (i, slot) in p.iter_mut().enumerate() {
        if !slot.used {
            slot.used = true;
            slot.r = 0;
            slot.len = 0;
            slot.nreaders = 1;
            slot.nwriters = 1;
            slot.data = [0; PIPE_CAP];
            return Ok(i);
        }
    }
    Err(())
}

pub fn close_reader(id: usize) {
    if id >= MAX_PIPES {
        return;
    }
    let p = &mut pipes_mut()[id];
    if !p.used {
        return;
    }
    if p.nreaders > 0 {
        p.nreaders -= 1;
    }
    maybe_free(id);
}

pub fn close_writer(id: usize) {
    if id >= MAX_PIPES {
        return;
    }
    let p = &mut pipes_mut()[id];
    if !p.used {
        return;
    }
    if p.nwriters > 0 {
        p.nwriters -= 1;
    }
    maybe_free(id);
}

/// Snapshot for poll/select/epoll (no wait).
pub fn poll_state(id: usize) -> Option<(bool, bool, bool, bool)> {
    if id >= MAX_PIPES {
        return None;
    }
    let p = &pipes_mut()[id];
    if !p.used {
        return None;
    }
    let can_read = p.len > 0 || p.nwriters == 0;
    let can_write = (p.len as usize) < PIPE_CAP || p.nreaders == 0;
    let rd_hup = p.nwriters == 0;
    let wr_err = p.nreaders == 0;
    Some((can_read, can_write, rd_hup, wr_err))
}

fn maybe_free(id: usize) {
    let p = &mut pipes_mut()[id];
    if p.nreaders == 0 && p.nwriters == 0 {
        p.used = false;
        p.len = 0;
        p.r = 0;
    }
}

/// Run a Ready child if any (parent/child pipe pattern).
fn schedule_child() {
    crate::syscalls::try_run_ready_child();
}

pub fn read(id: usize, buf: &mut [u8]) -> Result<usize, i32> {
    if id >= MAX_PIPES || buf.is_empty() {
        return Err(-22); // EINVAL
    }
    // Bound spins so a bug cannot hang forever.
    for _ in 0..2_000_000 {
        let p = &mut pipes_mut()[id];
        if !p.used {
            return Err(-9); // EBADF
        }
        if p.len > 0 {
            let n = (p.len as usize).min(buf.len());
            for i in 0..n {
                let idx = ((p.r as usize) + i) % PIPE_CAP;
                buf[i] = p.data[idx];
            }
            p.r = ((p.r as usize + n) % PIPE_CAP) as u16;
            p.len -= n as u16;
            return Ok(n);
        }
        if p.nwriters == 0 {
            return Ok(0); // EOF
        }
        schedule_child();
    }
    Err(-11) // EAGAIN
}

pub fn write(id: usize, data: &[u8]) -> Result<usize, i32> {
    if id >= MAX_PIPES {
        return Err(-22);
    }
    if data.is_empty() {
        return Ok(0);
    }
    let mut written = 0usize;
    for _ in 0..2_000_000 {
        let p = &mut pipes_mut()[id];
        if !p.used {
            return Err(-9);
        }
        if p.nreaders == 0 {
            // SIGPIPE would go here; return EPIPE
            return if written > 0 {
                Ok(written)
            } else {
                Err(-32) // EPIPE
            };
        }
        let space = PIPE_CAP - p.len as usize;
        if space > 0 {
            let n = space.min(data.len() - written);
            let wstart = (p.r as usize + p.len as usize) % PIPE_CAP;
            for i in 0..n {
                let idx = (wstart + i) % PIPE_CAP;
                p.data[idx] = data[written + i];
            }
            p.len += n as u16;
            written += n;
            if written == data.len() {
                return Ok(written);
            }
            continue;
        }
        schedule_child();
    }
    if written > 0 {
        Ok(written)
    } else {
        Err(-11)
    }
}
