//! Process Control Block (PCB) — all data about one process.

/// Process id (Unix-like; 0 reserved unused, 1 = init).
pub type Pid = i32;
/// User id of the owner.
pub type Uid = u32;

pub const MAX_PROCESSES: usize = 16;
pub const MAX_CHILDREN: usize = 8;
pub const PROC_SIG_QUEUE: usize = 16;
pub const MAX_SIGNALS: usize = 32;
/// Max mmap regions tracked per process (ld.so maps several libc segments).
pub const MAX_MMAPS: usize = 32;

/// One mmap region (page-aligned addr/len). File `MAP_SHARED` stores inode +
/// file offset so `munmap` / exec can write pages back.
#[derive(Clone, Copy)]
pub struct MmapRegion {
    pub used: bool,
    pub addr: u64,
    pub len: u64,
    /// `true` = `MAP_SHARED` file map (writeback on unmap).
    pub shared: bool,
    /// Ext2 inode for file-backed maps (`0` = anonymous / private snapshot).
    pub file_ino: u32,
    /// Page-aligned file offset passed to `mmap`.
    pub file_off: u64,
    /// Userspace length (not page-ceiled) — bytes to write back.
    pub file_len: u64,
}

impl MmapRegion {
    pub const fn empty() -> Self {
        Self {
            used: false,
            addr: 0,
            len: 0,
            shared: false,
            file_ino: 0,
            file_off: 0,
            file_len: 0,
        }
    }
}

/// Process status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ProcessState {
    /// Slot free
    Unused = 0,
    /// Runnable / currently selected
    Running = 1,
    /// Waiting for CPU
    Ready = 2,
    /// Exited; waiting for parent wait()
    Zombie = 3,
    /// Lightweight: shares address space with parent
    Thread = 4,
    /// Blocked in wait() or similar
    Sleeping = 5,
}

impl ProcessState {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcessState::Unused => "unused",
            ProcessState::Running => "running",
            ProcessState::Ready => "ready",
            ProcessState::Zombie => "zombie",
            ProcessState::Thread => "thread",
            ProcessState::Sleeping => "sleeping",
        }
    }
}

use super::trap::TrapFrame;

/// Full process structure.
#[derive(Clone, Copy)]
pub struct Process {
    pub used: bool,
    /// Unique task id (Linux **tid**). Also the value returned by `gettid`.
    pub pid: Pid,
    /// Thread-group id (Linux **tgid**). `getpid` returns this.
    /// For a normal process `tgid == pid`. Threads share the leader's tgid.
    pub tgid: Pid,
    pub state: ProcessState,
    /// Parent PID (−1 if none)
    pub parent: Pid,
    /// Process group id (job control). Fork inherits; `setsid`/`setpgid` change it.
    pub pgid: Pid,
    /// Session id. Fork inherits; `setsid` makes a new session (`sid == pid`).
    pub sid: Pid,
    /// Controlling tty: 0 = none, 1 = console (PTY ids later).
    pub ctty: i32,
    pub children: [Pid; MAX_CHILDREN],
    pub nchildren: usize,
    pub uid: Uid,
    /// Exit status (for zombies / wait) — raw code from exit(status)
    pub exit_code: i32,
    /// User pointer for `set_tid_address` / `CLONE_CHILD_CLEARTID` (cleared on exit later).
    pub clear_child_tid: u64,
    /// CR3 is shared with other tasks (`CLONE_VM`) — do not free_mm until last user.
    pub mm_shared: bool,
    /// Process-table slot of the FD table this task uses (`CLONE_FILES` share).
    /// Usually equal to this task's own slot index.
    pub files_slot: usize,

    /// Stack region (virtual)
    pub stack_base: u64,
    pub stack_size: u64,
    /// Heap region (virtual)
    pub heap_base: u64,
    pub heap_size: u64,

    /// Anonymous mmap regions (Linux mmap MAP_ANONYMOUS).
    pub mmaps: [MmapRegion; MAX_MMAPS],
    /// Next free VA for kernel-chosen mmap addresses (0 → default base).
    pub mmap_bump: u64,

    /// Current working directory (ext2 inode). Each process has its own pwd.
    pub cwd_inode: u32,

    /// User FS/GS base (TLS) — applied on context switch / enter user.
    pub fs_base: u64,
    pub gs_base: u64,

    /// Root of this process's page tables (PML4 physical address / CR3).
    /// `0` means “use kernel reference CR3” (before mm is bound to the task).
    pub cr3: u64,

    /// Top of this process’s kernel stack (TSS RSP0 / syscall). `0` → boot stack.
    pub kstack_top: u64,

    /// Full trap frame if the task was interrupted in user mode (IRQ preempt).
    pub trap: TrapFrame,
    /// True after a full [`TrapFrame`] was saved (IRQ or synthetic resume).
    pub trap_valid: bool,
    /// Entered via `enter_user_mode` nest (wait/run). If false, was IRQ-resumed
    /// or only Ready after fork — exit must not `return_from_user`.
    pub entered_via_nest: bool,

    /// Lightweight user entry (fork / first schedule via wait).
    pub user_rip: u64,
    pub user_rsp: u64,
    pub user_rflags: u64,
    /// rax on (re)entry — 0 for child after fork
    pub user_rax: u64,

    /// Pending signals (queue), delivered on tick / syscall return
    pub sig_queue: [u32; PROC_SIG_QUEUE],
    pub sig_head: usize,
    pub sig_tail: usize,
    pub sig_len: usize,
    pub sig_handlers: [usize; MAX_SIGNALS],
    pub sig_ignore: [bool; MAX_SIGNALS],
    /// Blocked signal mask (bits 1..63 for signals 1..63).
    pub sig_blocked: u64,
    /// Deferred fatal signal (set from IRQ/TTY); processed in process context.
    /// 0 = none; otherwise Linux signal number (e.g. SIGINT=2).
    pub force_fatal_sig: u32,
    /// Saved user context while running a signal handler (`rt_sigreturn`).
    pub sig_restore: TrapFrame,
    /// True while executing a user signal handler (not nested for this slice).
    pub sig_in_handler: bool,

    /// Name for debugging / `prctl(PR_SET_NAME)` (`TASK_COMM_LEN` = 16).
    pub name: [u8; 16],
    /// `prctl(PR_SET_DUMPABLE)` — 0/1/2; default 1 (Linux `SUID_DUMP_USER`).
    pub dumpable: u8,
    /// `prctl(PR_SET_NO_NEW_PRIVS)` — once 1, stays 1 (no setuid effect here).
    pub no_new_privs: bool,
    /// `prctl(PR_SET_PDEATHSIG)` — signal delivered when the parent task exits.
    /// 0 = none. Not inherited across `fork`/`clone` (Linux).
    pub pdeathsig: u32,
}

impl Process {
    pub const fn empty() -> Self {
        Self {
            used: false,
            pid: 0,
            tgid: 0,
            state: ProcessState::Unused,
            parent: -1,
            pgid: 0,
            sid: 0,
            ctty: 0,
            children: [-1; MAX_CHILDREN],
            nchildren: 0,
            uid: 0,
            exit_code: 0,
            clear_child_tid: 0,
            mm_shared: false,
            files_slot: 0,
            stack_base: 0,
            stack_size: 0,
            heap_base: 0,
            heap_size: 0,
            mmaps: [MmapRegion::empty(); MAX_MMAPS],
            mmap_bump: 0,
            cwd_inode: 2, // ext2 root
            fs_base: 0,
            gs_base: 0,
            cr3: 0,
            kstack_top: 0,
            trap: TrapFrame::zero(),
            trap_valid: false,
            entered_via_nest: false,
            user_rip: 0,
            user_rsp: 0,
            user_rflags: 0x202,
            user_rax: 0,
            sig_queue: [0; PROC_SIG_QUEUE],
            sig_head: 0,
            sig_tail: 0,
            sig_len: 0,
            sig_handlers: [0; MAX_SIGNALS],
            sig_ignore: [false; MAX_SIGNALS],
            sig_blocked: 0,
            force_fatal_sig: 0,
            sig_restore: TrapFrame::zero(),
            sig_in_handler: false,
            name: [0; 16],
            dumpable: 1,
            no_new_privs: false,
            pdeathsig: 0,
        }
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 16];
        for (i, b) in s.bytes().take(15).enumerate() {
            self.name[i] = b;
        }
    }

    pub fn name_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..len]).unwrap_or("?")
    }

    pub fn push_signal(&mut self, sig: u32) -> bool {
        if self.sig_len >= PROC_SIG_QUEUE {
            return false;
        }
        self.sig_queue[self.sig_tail] = sig;
        self.sig_tail = (self.sig_tail + 1) % PROC_SIG_QUEUE;
        self.sig_len += 1;
        true
    }

    pub fn pop_signal(&mut self) -> Option<u32> {
        if self.sig_len == 0 {
            return None;
        }
        let s = self.sig_queue[self.sig_head];
        self.sig_head = (self.sig_head + 1) % PROC_SIG_QUEUE;
        self.sig_len -= 1;
        Some(s)
    }
}
