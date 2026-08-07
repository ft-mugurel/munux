//! Minimal level-triggered epoll (create1 / ctl / wait).

use super::poll;
use super::{with_current, File, FD_MAX};
use crate::fs::vcore::{self, FileData};

pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

const MAX_EPOLL: usize = 8;
const MAX_WATCH: usize = 16;

#[derive(Clone, Copy)]
struct Watch {
    used: bool,
    fd: i32,
    events: u32,
    data: u64,
}

impl Watch {
    const fn empty() -> Self {
        Self {
            used: false,
            fd: -1,
            events: 0,
            data: 0,
        }
    }
}

struct Epoll {
    used: bool,
    watches: [Watch; MAX_WATCH],
}

impl Epoll {
    const fn empty() -> Self {
        Self {
            used: false,
            watches: [Watch::empty(); MAX_WATCH],
        }
    }
}

static mut INST: [Epoll; MAX_EPOLL] = [
    Epoll::empty(),
    Epoll::empty(),
    Epoll::empty(),
    Epoll::empty(),
    Epoll::empty(),
    Epoll::empty(),
    Epoll::empty(),
    Epoll::empty(),
];

fn inst_mut() -> &'static mut [Epoll; MAX_EPOLL] {
    unsafe { &mut *core::ptr::addr_of_mut!(INST) }
}

pub fn alloc() -> Result<usize, ()> {
    let t = inst_mut();
    for (i, e) in t.iter_mut().enumerate() {
        if !e.used {
            e.used = true;
            e.watches = [Watch::empty(); MAX_WATCH];
            return Ok(i);
        }
    }
    Err(())
}

pub fn free(id: usize) {
    if id >= MAX_EPOLL {
        return;
    }
    inst_mut()[id] = Epoll::empty();
}

fn is_epoll_file(f: &File) -> Option<usize> {
    if f.data.fops_id == vcore::FOPS_EPOLL {
        Some(f.data.private as usize)
    } else {
        None
    }
}

pub fn create_fd(flags: u32) -> Result<usize, i64> {
    let _ = flags; // CLOEXEC ignored
    let id = alloc().map_err(|_| 24i64)?; // EMFILE
    let file = File::from_vfs(FileData {
        pos: 0,
        readable: false,
        writable: false,
        private: id as u64,
        is_dir: false,
        fops_id: vcore::FOPS_EPOLL,
    });
    match with_current(|t| t.install(file)) {
        Ok(fd) => Ok(fd),
        Err(_) => {
            free(id);
            Err(24)
        }
    }
}

pub fn ctl(epfd: usize, op: i32, fd: i32, events: u32, data: u64) -> Result<(), i64> {
    if epfd >= FD_MAX || fd < 0 || (fd as usize) >= FD_MAX {
        return Err(9); // EBADF
    }
    if fd as usize == epfd {
        return Err(22); // EINVAL — watch self
    }
    let eid = with_current(|t| {
        let e = t.get(epfd).ok_or(9i64)?;
        is_epoll_file(e).ok_or(22i64)
    })?;
    if eid >= MAX_EPOLL || !inst_mut()[eid].used {
        return Err(9);
    }
    // Target must exist for ADD/MOD.
    if op != EPOLL_CTL_DEL {
        let ok = with_current(|t| t.get(fd as usize).map(|f| f.is_open()).unwrap_or(false));
        if !ok {
            return Err(9);
        }
    }
    let e = &mut inst_mut()[eid];
    match op {
        EPOLL_CTL_ADD => {
            for w in e.watches.iter() {
                if w.used && w.fd == fd {
                    return Err(17); // EEXIST
                }
            }
            for w in e.watches.iter_mut() {
                if !w.used {
                    w.used = true;
                    w.fd = fd;
                    w.events = events;
                    w.data = data;
                    return Ok(());
                }
            }
            Err(12) // ENOMEM
        }
        EPOLL_CTL_MOD => {
            for w in e.watches.iter_mut() {
                if w.used && w.fd == fd {
                    w.events = events;
                    w.data = data;
                    return Ok(());
                }
            }
            Err(2) // ENOENT
        }
        EPOLL_CTL_DEL => {
            for w in e.watches.iter_mut() {
                if w.used && w.fd == fd {
                    *w = Watch::empty();
                    return Ok(());
                }
            }
            Err(2)
        }
        _ => Err(22),
    }
}

/// Fill `out` with ready events. Returns count.
pub fn collect(epfd: usize, out: &mut [(u32, u64)]) -> Result<usize, i64> {
    if epfd >= FD_MAX {
        return Err(9);
    }
    let eid = with_current(|t| {
        let e = t.get(epfd).ok_or(9i64)?;
        is_epoll_file(e).ok_or(22i64)
    })?;
    if eid >= MAX_EPOLL || !inst_mut()[eid].used {
        return Err(9);
    }
    let mut n = 0usize;
    // Copy watches first (revents may take fd lock).
    let mut snap = [(false, -1i32, 0u32, 0u64); MAX_WATCH];
    {
        let e = &inst_mut()[eid];
        for (i, w) in e.watches.iter().enumerate() {
            snap[i] = (w.used, w.fd, w.events, w.data);
        }
    }
    for &(used, fd, ev, data) in snap.iter() {
        if !used || n >= out.len() {
            continue;
        }
        let want = (ev as u16) | poll::POLLIN | poll::POLLOUT;
        let got = poll::revents(fd, want) as u32;
        let mask = ev | (poll::POLLERR as u32) | (poll::POLLHUP as u32) | (poll::POLLNVAL as u32);
        let rev = got & mask;
        if rev != 0 {
            out[n] = (rev, data);
            n += 1;
        }
    }
    Ok(n)
}
