//! FD readiness for `poll` / `select` / `epoll` (level-triggered).

use super::pipe;
use super::{with_current, FD_MAX, File};
use crate::fs::vcore;
use crate::interrupts::keyboard::init as kbd;

pub const POLLIN: u16 = 0x0001;
pub const POLLPRI: u16 = 0x0002;
pub const POLLOUT: u16 = 0x0004;
pub const POLLERR: u16 = 0x0008;
pub const POLLHUP: u16 = 0x0010;
pub const POLLNVAL: u16 = 0x0020;

fn file_revents(file: &File, want: u16) -> u16 {
    let f = &file.data;
    let mut got = 0u16;
    match f.fops_id {
        vcore::FOPS_NONE => return POLLNVAL,
        vcore::FOPS_PIPE_R => {
            match pipe::poll_state(f.private as usize) {
                None => return POLLNVAL,
                Some((rd, _, hup, _)) => {
                    if rd {
                        got |= POLLIN;
                    }
                    if hup {
                        got |= POLLHUP;
                    }
                }
            }
        }
        vcore::FOPS_PIPE_W => {
            match pipe::poll_state(f.private as usize) {
                None => return POLLNVAL,
                Some((_, wr, _, err)) => {
                    if wr {
                        got |= POLLOUT;
                    }
                    if err {
                        got |= POLLERR;
                    }
                }
            }
        }
        vcore::FOPS_CONSOLE => {
            if f.readable && kbd::buffered_len() > 0 {
                got |= POLLIN;
            }
            if f.writable {
                got |= POLLOUT;
            }
        }
        vcore::FOPS_EPOLL => {
            // epoll fd is pollable for POLLIN when any watch is ready — skip (avoid recursion)
            if f.writable || f.readable {
                got |= POLLOUT;
            }
        }
        _ => {
            // Regular files, dirs, proc, ram, null/zero, blk: always ready.
            if f.readable || f.is_dir {
                got |= POLLIN;
            }
            if f.writable {
                got |= POLLOUT;
            }
            if !f.readable && !f.writable && !f.is_dir {
                got |= POLLOUT | POLLIN;
            }
        }
    }
    let report = want | POLLERR | POLLHUP | POLLNVAL;
    got & report
}

/// `revents` for one fd. Negative fd → 0 (poll ignores). Closed → POLLNVAL.
pub fn revents(fd: i32, events: u16) -> u16 {
    if fd < 0 {
        return 0;
    }
    let fd = fd as usize;
    if fd >= FD_MAX {
        return POLLNVAL;
    }
    with_current(|t| match t.get(fd) {
        Some(f) if f.is_open() => file_revents(f, events),
        _ => POLLNVAL,
    })
}

/// True if any requested bit (plus ERR/HUP/NVAL) is set.
pub fn is_ready(fd: i32, events: u16) -> bool {
    revents(fd, events) != 0
}
