//! Process management — PCBs, spawn/exit/wait, signals, memory, cooperative sched.

pub mod clone;
pub mod fork;
pub mod futex;
pub mod kstack;
pub mod memory;
pub mod pcb;
pub mod sched;
pub mod signal_queue;
pub mod sys;
pub mod table;
pub mod trap;

pub use trap::TrapFrame;

pub use clone::clone_from_user;
pub use fork::{fork_from_user, UserFrame};
pub use memory::{
    clear_mmaps, proc_brk, proc_mmap, proc_mprotect, proc_munmap, set_brk_start, MAP_ANONYMOUS,
    MAP_FIXED, MAP_PRIVATE, PROT_READ, PROT_WRITE,
};
pub use pcb::{Pid, Process, ProcessState, Uid, MAX_PROCESSES};
pub use sys::{
    begin_user_task, exit_group, exit_user, getpid, getppid, gettid, getuid, reap_any_child,
    waitpid, waitpid_opts,
};
pub use table::{current_index, current_pid, for_each_process, process_count, with_current};

/// Private user stack region for the current process, if any.
///
/// After Phase 1 fork, children keep the classic stack VA window on a private
/// CR3 (`stack_base` / `stack_size` set). `0` means “use default ELF stack top”
/// (initial shell). Exec rebuilds argv in this region when set.
pub fn current_stack_region() -> Option<(u64, u64)> {
    table::with_current(|p| {
        if p.stack_base != 0 && p.stack_size != 0 {
            Some((p.stack_base, p.stack_size))
        } else {
            None
        }
    })
    .flatten()
}

/// Per-process working directory (ext2 inode).
pub fn get_cwd_inode() -> u32 {
    table::with_current(|p| p.cwd_inode).unwrap_or(2)
}

pub fn set_cwd_inode(ino: u32) {
    let _ = table::with_current(|p| {
        p.cwd_inode = ino;
    });
}

/// Load the current process's FS/GS bases into the CPU (TLS).
pub fn apply_tls() {
    let (fs, gs) = table::with_current(|p| (p.fs_base, p.gs_base)).unwrap_or((0, 0));
    crate::x86::msr::set_fs_base(fs);
    crate::x86::msr::set_gs_base(gs);
}

/// Zero TLS for the current process (exit / execve new image).
pub fn clear_tls() {
    let _ = table::with_current(|p| {
        p.fs_base = 0;
        p.gs_base = 0;
    });
    crate::x86::msr::set_fs_base(0);
    crate::x86::msr::set_gs_base(0);
}

/// Store FS base for the current process (CPU updated on apply_tls / sysret).
pub fn set_fs_base(base: u64) {
    let _ = table::with_current(|p| {
        p.fs_base = base;
    });
}

/// Store GS base for the current process (CPU updated on apply_tls / sysret).
pub fn set_gs_base(base: u64) {
    let _ = table::with_current(|p| {
        p.gs_base = base;
    });
}

pub fn get_fs_base_saved() -> u64 {
    table::with_current(|p| p.fs_base).unwrap_or(0)
}

pub fn get_gs_base_saved() -> u64 {
    table::with_current(|p| p.gs_base).unwrap_or(0)
}

/// Boot: create process table with init (pid 1).
pub fn init_processes() {
    table::init_table();
    let kcr3 = crate::memory::kernel_cr3();
    crate::console::print("process: kinit pid=");
    crate::console::write_u64(current_pid() as u64);
    crate::console::print(" uid=");
    crate::console::write_u64(getuid() as u64);
    crate::console::print(" cr3=");
    crate::console::write_hex64(kcr3);
    crate::console::println("");
}

/// Called from timer IRQ each tick — signals + scheduler tick.
pub fn on_cpu_tick() {
    // Ctrl-C from keyboard IRQ is deferred here (not delivered in IRQ).
    crate::tty::deliver_pending_sigint();
    signal_queue::deliver_pending_on_tick();
    sched::on_timer_tick(crate::interrupts::timer::ticks());
}
