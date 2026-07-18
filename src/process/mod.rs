//! Process management — PCBs, spawn/exit/wait, signals, sockets, memory.
//!
//! U5: real PIDs, zombie exit, wait4, getpid/getppid, cwd per process.
//! User tasks (`/bin/sh` at U8 boot, or `run`/`user`) are children of kinit (pid 1).

pub mod fork;
pub mod memory;
pub mod pcb;
pub mod signal_queue;
pub mod socket;
pub mod sys;
pub mod table;

pub use fork::{fork, fork_from_user, switch_to, take_ready_child, UserFrame};
pub use memory::{proc_read_mem, proc_sbrk, proc_write_mem};
pub use pcb::{Pid, Process, ProcessState, Uid, MAX_PROCESSES};
pub use socket::{socket_close, socket_connect, socket_create, socket_recv, socket_send};
pub use sys::{
    begin_user_task, exit, exit_user, getpid, getppid, getuid, kill, reap_any_child, setuid,
    signal, wait, waitpid,
};
pub use table::{current_pid, for_each_process, process_count, with_current};

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
    crate::console::print("process: kinit pid=");
    crate::console::write_u64(current_pid() as u64);
    crate::console::print(" uid=");
    crate::console::write_u64(getuid() as u64);
    crate::console::println("");
}

/// Called from timer IRQ each tick — deliver per-process signal queues.
pub fn on_cpu_tick() {
    signal_queue::deliver_pending_on_tick();
}
