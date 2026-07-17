//! Minimal **socket-like** IPC helpers between processes (Unix-inspired, not full BSD).
//!
//! Each "socket" is a small message buffer owned by one process, connectable to a peer.

use super::pcb::Pid;
use super::table;

pub const MAX_SOCKETS: usize = 32;
pub const SOCK_BUF: usize = 256;

#[derive(Clone, Copy)]
pub struct Socket {
    pub used: bool,
    pub owner: Pid,
    pub peer: Pid, // -1 if unbound
    pub buf: [u8; SOCK_BUF],
    pub len: usize,
    pub closed: bool,
}

static mut SOCKS: [Socket; MAX_SOCKETS] = [Socket {
    used: false,
    owner: -1,
    peer: -1,
    buf: [0; SOCK_BUF],
    len: 0,
    closed: false,
}; MAX_SOCKETS];

/// Create a socket for the current process. Returns fd (index) or -1.
pub fn socket_create() -> i32 {
    let owner = table::current_pid();
    unsafe {
        for i in 0..MAX_SOCKETS {
            if !SOCKS[i].used {
                SOCKS[i] = Socket {
                    used: true,
                    owner,
                    peer: -1,
                    buf: [0; SOCK_BUF],
                    len: 0,
                    closed: false,
                };
                return i as i32;
            }
        }
    }
    -1
}

/// Connect `fd` to peer process (bidirectional link of two sockets if peer has one free).
pub fn socket_connect(fd: i32, peer_pid: Pid) -> i32 {
    if fd < 0 || fd as usize >= MAX_SOCKETS {
        return -1;
    }
    let me = table::current_pid();
    if table::find_pid(peer_pid).is_none() {
        return -1;
    }
    unsafe {
        let s = &mut SOCKS[fd as usize];
        if !s.used || s.owner != me {
            return -1;
        }
        s.peer = peer_pid;
        // Try attach peer's first free socket toward us
        for j in 0..MAX_SOCKETS {
            if SOCKS[j].used && SOCKS[j].owner == peer_pid && SOCKS[j].peer < 0 {
                SOCKS[j].peer = me;
                break;
            }
        }
    }
    0
}

/// Send bytes to peer (copied into peer's socket buffer).
pub fn socket_send(fd: i32, data: &[u8]) -> i32 {
    if fd < 0 || fd as usize >= MAX_SOCKETS {
        return -1;
    }
    let me = table::current_pid();
    unsafe {
        let s = &SOCKS[fd as usize];
        if !s.used || s.owner != me || s.peer < 0 {
            return -1;
        }
        let peer = s.peer;
        // Find a socket owned by peer that peers back to us (or any peer socket)
        for j in 0..MAX_SOCKETS {
            if SOCKS[j].used && SOCKS[j].owner == peer {
                let dst = &mut SOCKS[j];
                let n = data.len().min(SOCK_BUF - dst.len);
                if n == 0 {
                    return 0;
                }
                dst.buf[dst.len..dst.len + n].copy_from_slice(&data[..n]);
                dst.len += n;
                return n as i32;
            }
        }
    }
    -1
}

/// Receive bytes from this socket's buffer.
pub fn socket_recv(fd: i32, out: &mut [u8]) -> i32 {
    if fd < 0 || fd as usize >= MAX_SOCKETS {
        return -1;
    }
    let me = table::current_pid();
    unsafe {
        let s = &mut SOCKS[fd as usize];
        if !s.used || s.owner != me {
            return -1;
        }
        let n = out.len().min(s.len);
        if n == 0 {
            return 0;
        }
        out[..n].copy_from_slice(&s.buf[..n]);
        // shift remaining
        let rest = s.len - n;
        for i in 0..rest {
            s.buf[i] = s.buf[n + i];
        }
        s.len = rest;
        n as i32
    }
}

/// Close socket.
pub fn socket_close(fd: i32) -> i32 {
    if fd < 0 || fd as usize >= MAX_SOCKETS {
        return -1;
    }
    let me = table::current_pid();
    unsafe {
        let s = &mut SOCKS[fd as usize];
        if !s.used || s.owner != me {
            return -1;
        }
        s.used = false;
        s.closed = true;
    }
    0
}
