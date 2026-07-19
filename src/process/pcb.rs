//! Process Control Block (PCB) — all data about one process.

/// Process id (Unix-like; 0 reserved unused, 1 = init).
pub type Pid = i32;
/// User id of the owner.
pub type Uid = u32;

pub const MAX_PROCESSES: usize = 16;
pub const MAX_CHILDREN: usize = 8;
pub const PROC_SIG_QUEUE: usize = 16;
pub const MAX_SIGNALS: usize = 32;

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

/// Saved CPU context for future kernel-mode switching (not used for ring-3 yet).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CpuContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rip: u64,
    pub rflags: u64,
    pub rsp: u64,
}

impl CpuContext {
    pub const fn zero() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rip: 0,
            rflags: 0x202, // IF set
            rsp: 0,
        }
    }
}

/// Full process structure.
#[derive(Clone, Copy)]
pub struct Process {
    pub used: bool,
    pub pid: Pid,
    pub state: ProcessState,
    /// Parent PID (−1 if none)
    pub parent: Pid,
    pub children: [Pid; MAX_CHILDREN],
    pub nchildren: usize,
    pub uid: Uid,
    /// Exit status (for zombies / wait) — raw code from exit(status)
    pub exit_code: i32,

    /// Stack region (virtual) — reserved for future per-process stacks
    pub stack_base: u64,
    pub stack_size: u64,
    /// Heap region (virtual)
    pub heap_base: u64,
    pub heap_size: u64,

    /// Current working directory (ext2 inode). Each process has its own pwd.
    pub cwd_inode: u32,

    /// User FS/GS base (TLS) — applied on context switch / enter user.
    pub fs_base: u64,
    pub gs_base: u64,

    pub ctx: CpuContext,

    /// Saved user ring-3 context (U6 fork / cooperative schedule).
    pub user_rip: u64,
    pub user_rsp: u64,
    pub user_rflags: u64,
    /// rax on (re)entry — 0 for child after fork
    pub user_rax: u64,

    /// Pending signals (queue), delivered on next CPU tick
    pub sig_queue: [u32; PROC_SIG_QUEUE],
    pub sig_head: usize,
    pub sig_tail: usize,
    pub sig_len: usize,
    pub sig_handlers: [usize; MAX_SIGNALS],
    pub sig_ignore: [bool; MAX_SIGNALS],

    /// Name for debugging
    pub name: [u8; 16],
}

impl Process {
    pub const fn empty() -> Self {
        Self {
            used: false,
            pid: 0,
            state: ProcessState::Unused,
            parent: -1,
            children: [-1; MAX_CHILDREN],
            nchildren: 0,
            uid: 0,
            exit_code: 0,
            stack_base: 0,
            stack_size: 0,
            heap_base: 0,
            heap_size: 0,
            cwd_inode: 2, // ext2 root
            fs_base: 0,
            gs_base: 0,
            ctx: CpuContext::zero(),
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
            name: [0; 16],
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
