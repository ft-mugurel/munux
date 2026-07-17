//! Process management — PCBs, fork/exit/wait, signals, sockets, memory.
//!
//! Also requires a **timer tick** (PIT) so queued process signals deliver
//! on the next CPU tick.

pub mod fork;
pub mod memory;
pub mod pcb;
pub mod signal_queue;
pub mod socket;
pub mod sys;
pub mod table;

pub use fork::{fork, switch_to};
pub use memory::{proc_read_mem, proc_sbrk, proc_write_mem};
pub use pcb::{Pid, Process, ProcessState, Uid, MAX_PROCESSES};
pub use socket::{socket_close, socket_connect, socket_create, socket_recv, socket_send};
pub use sys::{exit, getuid, kill, setuid, signal, wait, waitpid};
pub use table::{current_pid, for_each_process, process_count};

/// Per-process working directory (ext2 inode).
pub fn get_cwd_inode() -> u32 {
    table::with_current(|p| p.cwd_inode).unwrap_or(2)
}

pub fn set_cwd_inode(ino: u32) {
    let _ = table::with_current(|p| {
        p.cwd_inode = ino;
    });
}

/// Boot: create process table with init (pid 1).
pub fn init_processes() {
    table::init_table();
    crate::println!("process: init pid={} uid={}", current_pid(), getuid());
}

/// Called from timer IRQ each tick — deliver per-process signal queues.
pub fn on_cpu_tick() {
    signal_queue::deliver_pending_on_tick();
}
