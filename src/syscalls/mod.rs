//! System calls via `syscall` / `sysret` and ring-3 demo.

use core::arch::asm;

use crate::console;
use crate::fd;
use crate::gdt::{self, STAR_KERNEL_CS, STAR_USER_BASE, USER_CODE_SELECTOR, USER_DATA_SELECTOR};
use crate::gdt::tss;
use crate::memory::pmm::FRAME_SIZE;

/// Linux **x86_64** syscall numbers (see `arch/x86/entry/syscalls/syscall_64.tbl`).
/// Using Linux numbers is required so static Linux binaries can target munux later.
pub mod num {
    // Implemented / reserved with Linux numbers:
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const CLOSE: u64 = 3;
    pub const READV: u64 = 19; // musl stdio (fread)
    pub const WRITEV: u64 = 20; // musl stdio (printf)
    pub const IOCTL: u64 = 16; // musl TIOCGWINSZ probe
    pub const STAT: u64 = 4; // busybox ls (classic x86_64)
    pub const FSTAT: u64 = 5;
    pub const LSTAT: u64 = 6;
    pub const LSEEK: u64 = 8;
    /// BusyBox `cat` prefers sendfile(out, in, …) before falling back to read/write.
    pub const SENDFILE: u64 = 40;
    pub const RT_SIGACTION: u64 = 13;
    pub const RT_SIGPROCMASK: u64 = 14;
    pub const RT_SIGRETURN: u64 = 15;
    pub const KILL: u64 = 62;
    pub const TKILL: u64 = 200;
    pub const TGKILL: u64 = 234;
    pub const GETUID: u64 = 102;
    pub const GETGID: u64 = 104;
    pub const SETUID: u64 = 105;
    pub const SETGID: u64 = 106;
    pub const GETEUID: u64 = 107;
    pub const GETEGID: u64 = 108;
    pub const GETGROUPS: u64 = 115;
    pub const GETPID: u64 = 39;
    pub const CLONE: u64 = 56;
    pub const CLONE3: u64 = 435;
    pub const FORK: u64 = 57;
    pub const GETTID: u64 = 186;
    pub const EXECVE: u64 = 59;
    pub const EXIT: u64 = 60;
    pub const WAIT4: u64 = 61;
    pub const GETCWD: u64 = 79;
    pub const CHDIR: u64 = 80;
    pub const GETPPID: u64 = 110;
    pub const EXIT_GROUP: u64 = 231; // musl/glibc often use this
    pub const UNAME: u64 = 63;
    pub const GETDENTS64: u64 = 217;
    pub const PRCTL: u64 = 157; // process control (name, dumpable, nnp, pdeathsig)
    pub const ARCH_PRCTL: u64 = 158; // musl TLS
    pub const BRK: u64 = 12;
    pub const MMAP: u64 = 9;
    pub const MPROTECT: u64 = 10;
    pub const MUNMAP: u64 = 11;
    pub const SET_TID_ADDRESS: u64 = 218; // musl crt TLS/thread exit hook
    pub const FUTEX: u64 = 202;
    pub const GETTIMEOFDAY: u64 = 96;
    pub const CLOCK_GETTIME: u64 = 228;
    pub const FCNTL: u64 = 72;
    pub const OPENAT: u64 = 257; // modern libc; thin wrapper over open
    pub const NEWFSTATAT: u64 = 262; // fstatat
    // File create/remove (BusyBox touch/mkdir/rm/…):
    pub const ACCESS: u64 = 21;
    pub const PIPE: u64 = 22;
    pub const SELECT: u64 = 23;
    pub const DUP: u64 = 32;
    pub const DUP2: u64 = 33;
    pub const POLL: u64 = 7;
    pub const PIPE2: u64 = 293;
    pub const EPOLL_CREATE: u64 = 213;
    pub const EPOLL_WAIT: u64 = 232;
    pub const EPOLL_CTL: u64 = 233;
    pub const EPOLL_CREATE1: u64 = 291;
    pub const PPOLL: u64 = 271;
    pub const NANOSLEEP: u64 = 35;
    pub const RENAME: u64 = 82;
    pub const MKDIR: u64 = 83;
    pub const RMDIR: u64 = 84;
    pub const LINK: u64 = 86;
    pub const UNLINK: u64 = 87;
    pub const SYMLINK: u64 = 88;
    pub const READLINK: u64 = 89;
    pub const CHMOD: u64 = 90;
    pub const CHOWN: u64 = 92;
    pub const UMASK: u64 = 95;
    pub const SYNC: u64 = 162;
    pub const MKDIRAT: u64 = 258;
    pub const UNLINKAT: u64 = 263;
    pub const RENAMEAT: u64 = 264;
    pub const FCHMODAT: u64 = 268;
    pub const FACCESSAT: u64 = 269;
    pub const UTIMENSAT: u64 = 280;
    pub const FUTIMESAT: u64 = 261; // obsolete; busybox touch probes it
    pub const UTIMES: u64 = 235;
    pub const SYMLINKAT: u64 = 266;
    pub const READLINKAT: u64 = 267;
    pub const STATX: u64 = 332;
    /// Phase 8: loadable modules (Linux numbers).
    pub const INIT_MODULE: u64 = 175;
    pub const DELETE_MODULE: u64 = 176;
    /// Optional: load from open fd (modern insmod); not required if open+read+init_module.
    pub const FINIT_MODULE: u64 = 313;
    /// Linux execveat(dirfd, pathname, argv, envp, flags).
    pub const EXECVEAT: u64 = 322;
    pub const PREAD64: u64 = 17;
    pub const SET_ROBUST_LIST: u64 = 273;
    pub const GETRANDOM: u64 = 318;
    pub const PRLIMIT64: u64 = 302;
    pub const RSEQ: u64 = 334;
}

/// Linux-style: return `-errno` as `u64` bit pattern (negative i64).
#[allow(dead_code)]
mod errno {
    pub const EPERM: i64 = 1;
    pub const ENOENT: i64 = 2;
    pub const ENOEXEC: i64 = 8;
    pub const EBADF: i64 = 9;
    pub const ECHILD: i64 = 10;
    pub const EAGAIN: i64 = 11;
    pub const ENOMEM: i64 = 12;
    pub const EACCES: i64 = 13;
    pub const EFAULT: i64 = 14;
    pub const EISDIR: i64 = 21;
    pub const EINVAL: i64 = 22;
    pub const ENOTDIR: i64 = 20;
    pub const EEXIST: i64 = 17;
    pub const ENOSYS: i64 = 38;
    pub const ENAMETOOLONG: i64 = 36;
    pub const EMFILE: i64 = 24;
    pub const ERANGE: i64 = 34;
    pub const ENOTTY: i64 = 25;
    pub const ENOTEMPTY: i64 = 39;
    pub const ELOOP: i64 = 40;
    pub const ETIMEDOUT: i64 = 110;
    pub const EPIPE: i64 = 32;
    pub const EBUSY: i64 = 16;

    #[inline]
    pub fn neg(e: i64) -> u64 {
        (-e) as u64
    }
}

fn map_fd_err(e: fd::FdError) -> u64 {
    match e {
        fd::FdError::BadFd => errno::neg(errno::EBADF),
        fd::FdError::Fault => errno::neg(errno::EFAULT),
        fd::FdError::NoEnt => errno::neg(errno::ENOENT),
        fd::FdError::IsDir => errno::neg(errno::EISDIR),
        fd::FdError::NotDir => errno::neg(errno::ENOTDIR),
        fd::FdError::NoMem => errno::neg(errno::EMFILE),
        fd::FdError::Inval => errno::neg(errno::EINVAL),
        fd::FdError::Exist => errno::neg(errno::EEXIST),
        fd::FdError::Loop => errno::neg(errno::ELOOP),
    }
}

/// User demo load addresses (outside 1 GiB identity map → mapped with U/S=1).
const DEMO_CODE: u64 = 0x0000_0000_4000_0000;
const DEMO_STACK_PAGE: u64 = 0x0000_0000_4000_1000;
const DEMO_STACK_TOP: u64 = DEMO_STACK_PAGE + 0x1000;

/// Nested syscall stacks so wait4/execve → child does not clobber the outer
/// syscall frame (all would otherwise share one `syscall_kstack` top).
///
/// 64 KiB: FS write paths need headroom. Safe while `kernel_end` < 0x400000
/// (BusyBox ET_EXEC). The 2 MiB ELF load buffer is at high VA so it is not in
/// .bss (that alone used to push kernel_end into the BusyBox window).
const NEST_KSTACK_BYTES: usize = 64 * 1024;
const NEST_KSTACK_MAX: usize = 6;



#[repr(align(16))]
struct NestKStack {
    #[allow(dead_code)]
    bytes: [u8; NEST_KSTACK_BYTES],
}

static mut NEST_KSTACKS: [NestKStack; NEST_KSTACK_MAX] = [
    NestKStack {
        bytes: [0; NEST_KSTACK_BYTES],
    },
    NestKStack {
        bytes: [0; NEST_KSTACK_BYTES],
    },
    NestKStack {
        bytes: [0; NEST_KSTACK_BYTES],
    },
    NestKStack {
        bytes: [0; NEST_KSTACK_BYTES],
    },
    NestKStack {
        bytes: [0; NEST_KSTACK_BYTES],
    },
    NestKStack {
        bytes: [0; NEST_KSTACK_BYTES],
    },
];
/// Depth 0 = base TSS/kernel stack; 1.. = NEST_KSTACKS[depth-1]
static mut SYSCALL_STACK_DEPTH: usize = 0;

extern "C" {
    /// Enter ring 3; `user_rax` is initial RAX (0 after fork for child).
    fn enter_user_mode(entry: u64, user_rsp: u64, user_rax: u64);
    fn return_from_user() -> !;
    fn resume_user_trap(frame: *const crate::process::TrapFrame) -> !;
    fn set_syscall_kstack(rsp: u64);
    fn syscall_entry();
    static last_user_rip: u64;
    static last_user_rsp: u64;
    static last_user_rflags: u64;
    /// 6th syscall argument (user r9) saved at `syscall_entry`.
    static last_user_r9: u64;
    static last_user_rdi: u64;
    static last_user_rsi: u64;
    static last_user_rdx: u64;
    static last_user_r8: u64;
    static last_user_r10: u64;
    static last_user_rbx: u64;
    static last_user_rbp: u64;
    static last_user_r12: u64;
    static last_user_r13: u64;
    static last_user_r14: u64;
    static last_user_r15: u64;
    /// Optional user RDI for enter_user_mode (signal handler arg).
    static mut enter_user_rdi: u64;
}

/// Filled before `enter_user_mode` so clone children keep parent GPRs / TLS path.
#[no_mangle]
pub static mut enter_user_frame: crate::process::TrapFrame = crate::process::TrapFrame::zero();

fn child_trap_from_syscall(rip: u64, rsp: u64, rflags: u64, rax: u64) -> crate::process::TrapFrame {
    let mut t = crate::process::TrapFrame::from_user_entry(rip, rsp, rflags, rax);
    unsafe {
        t.rdi = core::ptr::read_volatile(core::ptr::addr_of!(last_user_rdi));
        t.rsi = core::ptr::read_volatile(core::ptr::addr_of!(last_user_rsi));
        t.rdx = core::ptr::read_volatile(core::ptr::addr_of!(last_user_rdx));
        t.r8 = core::ptr::read_volatile(core::ptr::addr_of!(last_user_r8));
        t.r9 = core::ptr::read_volatile(core::ptr::addr_of!(last_user_r9));
        t.r10 = core::ptr::read_volatile(core::ptr::addr_of!(last_user_r10));
        t.rbx = core::ptr::read_volatile(core::ptr::addr_of!(last_user_rbx));
        t.rbp = core::ptr::read_volatile(core::ptr::addr_of!(last_user_rbp));
        t.r12 = core::ptr::read_volatile(core::ptr::addr_of!(last_user_r12));
        t.r13 = core::ptr::read_volatile(core::ptr::addr_of!(last_user_r13));
        t.r14 = core::ptr::read_volatile(core::ptr::addr_of!(last_user_r14));
        t.r15 = core::ptr::read_volatile(core::ptr::addr_of!(last_user_r15));
        t.rcx = rip; // sysret-compatible
        t.r11 = rflags;
    }
    t
}

fn nest_stack_top(index: usize) -> u64 {
    unsafe {
        let base = core::ptr::addr_of!(NEST_KSTACKS[index]) as *const u8 as u64;
        base + NEST_KSTACK_BYTES as u64
    }
}

/// Push a fresh **syscall-only** nest stack for nested user entry.
///
/// Important: do **not** point TSS.RSP0 at the nest stack. IRQs from user use
/// RSP0; if that were the nest stack they would overwrite `enter_user_mode`'s
/// return frame. RSP0 stays on the per-process kernel stack.
fn push_syscall_stack() {
    unsafe {
        if SYSCALL_STACK_DEPTH >= NEST_KSTACK_MAX {
            return;
        }
        SYSCALL_STACK_DEPTH += 1;
        let top = nest_stack_top(SYSCALL_STACK_DEPTH - 1);
        set_syscall_kstack(top);
        // Keep TSS.rsp0 on this process's kstack (install_for_slot).
        let idx = crate::process::current_index();
        crate::gdt::tss::set_kernel_stack(crate::process::kstack::top_for_slot(idx));
    }
}

fn pop_syscall_stack() {
    unsafe {
        if SYSCALL_STACK_DEPTH == 0 {
            ensure_kstack_base();
            return;
        }
        SYSCALL_STACK_DEPTH -= 1;
        if SYSCALL_STACK_DEPTH == 0 {
            ensure_kstack_base();
        } else {
            let top = nest_stack_top(SYSCALL_STACK_DEPTH - 1);
            set_syscall_kstack(top);
            let idx = crate::process::current_index();
            crate::gdt::tss::set_kernel_stack(crate::process::kstack::top_for_slot(idx));
        }
    }
}

fn ensure_kstack_base() {
    // Restore this process’s own kernel stack (not always the boot stack).
    let idx = crate::process::current_index();
    crate::process::kstack::install_for_slot(idx);
}

/// Enter user with a private syscall stack for nested sessions.
fn enter_user_nested(entry: u64, user_rsp: u64, user_rax: u64) {
    push_syscall_stack();
    // Always load the current process CR3 before ring 3 (Phase 1b).
    if let Some(cr3) = crate::process::with_current(|p| p.cr3) {
        if cr3 != 0 {
            crate::memory::switch_mm(cr3);
        }
    }
    // Prefer a full saved trap (e.g. signal-handler inject) so rdi/etc. match.
    let (entry, user_rsp, user_rax, user_rdi, rflags) =
        crate::process::with_current(|p| {
            if p.trap_valid && p.trap.rip == entry {
                (
                    p.trap.rip,
                    p.trap.rsp,
                    p.trap.rax,
                    p.trap.rdi,
                    p.trap.rflags | 0x200,
                )
            } else {
                (entry, user_rsp, user_rax, 0u64, 0x202u64)
            }
        })
        .unwrap_or((entry, user_rsp, user_rax, 0, 0x202));

    // This task owns an enter_user_mode nest frame until exit.
    // Keep existing GPRs (clone child needs parent rdx/r8 for glibc clone3).
    let _ = crate::process::with_current(|p| {
        p.entered_via_nest = true;
        p.user_rip = entry;
        p.user_rsp = user_rsp;
        p.user_rax = user_rax;
        p.user_rflags = rflags;
        if p.trap_valid {
            p.trap.rip = entry;
            p.trap.rsp = user_rsp;
            p.trap.rax = user_rax;
            p.trap.rflags = rflags;
        } else {
            p.trap = crate::process::TrapFrame::from_user_entry(entry, user_rsp, rflags, user_rax);
            p.trap.rdi = user_rdi;
            p.trap_valid = true;
        }
    });
    // Pass first user arg (signal number for handlers).
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(enter_user_rdi), user_rdi);
    }
    crate::process::apply_tls();
    unsafe {
        let tr = crate::process::with_current(|p| p.trap).unwrap_or(
            crate::process::TrapFrame::from_user_entry(entry, user_rsp, rflags, user_rax),
        );
        enter_user_frame = tr;
        enter_user_frame.rip = entry;
        enter_user_frame.rsp = user_rsp;
        enter_user_frame.rax = user_rax;
        enter_user_mode(entry, user_rsp, user_rax);
    }
    pop_syscall_stack();
}

/// Resume the **current** process after an IRQ-resumed task exited.
///
/// Prefer a saved trap frame (mid-user after preempt). If none, fall back to
/// popping an `enter_user_mode` nest frame when one exists.
fn resume_current_from_trap() -> ! {
    ensure_kstack_base();
    crate::process::apply_tls();

    let (trap_valid, frame) = crate::process::with_current(|p| {
        if p.trap_valid {
            (true, p.trap)
        } else if p.user_rip >= 0x1000 {
            (
                true,
                crate::process::TrapFrame::from_user_entry(
                    p.user_rip,
                    p.user_rsp,
                    p.user_rflags,
                    p.user_rax,
                ),
            )
        } else {
            (false, crate::process::TrapFrame::zero())
        }
    })
    .unwrap_or((false, crate::process::TrapFrame::zero()));

    if trap_valid && frame.rip >= 0x1000 && frame.is_user() {
        static mut RESUME_BUF: crate::process::TrapFrame = crate::process::TrapFrame::zero();
        unsafe {
            RESUME_BUF = frame;
            resume_user_trap(core::ptr::addr_of!(RESUME_BUF));
        }
    }

    // No usable trap — try nest return (e.g. back to wait/run).
    extern "C" {
        fn get_enter_nest_depth() -> u64;
    }
    if unsafe { get_enter_nest_depth() } > 0 {
        unsafe {
            return_from_user();
        }
    }

    // Last resort: stay in kernel (idle).
    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

/// MSR helpers
unsafe fn wrmsr(msr: u32, value: u64) {
    let lo = value as u32;
    let hi = (value >> 32) as u32;
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") lo,
        in("edx") hi,
        options(nostack, preserves_flags)
    );
}

unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
        options(nomem, nostack, preserves_flags)
    );
    ((hi as u64) << 32) | (lo as u64)
}

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;
const EFER_SCE: u64 = 1;

/// Arm `syscall` (STAR / LSTAR / FMASK / EFER.SCE).
pub fn init_syscalls() {
    unsafe {
        // STAR: kernel CS in 47:32, user base in 63:48
        let star = ((STAR_USER_BASE as u64) << 48) | ((STAR_KERNEL_CS as u64) << 32);
        wrmsr(IA32_STAR, star);
        wrmsr(IA32_LSTAR, syscall_entry as usize as u64);
        // Clear IF (bit 9) among others on entry — 0x200
        wrmsr(IA32_FMASK, 0x200);
        let efer = rdmsr(IA32_EFER) | EFER_SCE;
        wrmsr(IA32_EFER, efer);
    }
    let _ = (USER_CODE_SELECTOR, USER_DATA_SELECTOR);
    console::println("syscall: Linux x86_64 numbers + STAR/LSTAR (SCE)");
}

/// C ABI from assembly.
#[no_mangle]
pub extern "C" fn syscall_dispatch(
    num: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> u64 {
    // Never run kernel code with a user TLS base in FS/GS. User SET_FS stays
    // only on the PCB until we restore it for sysret (see end of this fn).
    crate::x86::msr::set_fs_base(0);
    crate::x86::msr::set_gs_base(0);
    // Keep software page-table root aligned with the current process (Phase 1b).
    if let Some(cr3) = crate::process::with_current(|p| p.cr3) {
        if cr3 != 0 {
            crate::memory::switch_mm(cr3);
        }
    }

    // TTY Ctrl-C + deferred fatal signals (process context only).
    crate::tty::deliver_pending_sigint();
    if let Some(sig) = crate::tty::take_force_fatal() {
        fatal_signal_exit(sig);
    }
    // Deliver any pending fatal defaults for *this* task before handling a
    // new syscall (cheap; user handlers not framed yet).
    crate::process::signal_queue::deliver_pending_current();

    let ret = match num {
        num::READ => sys_read(a1, a2, a3),
        num::WRITE => sys_write(a1, a2, a3),
        num::READV => sys_readv(a1, a2, a3),
        num::WRITEV => sys_writev(a1, a2, a3),
        num::IOCTL => sys_ioctl(a1, a2, a3),
        num::OPEN => sys_open(a1, a2, a3),
        num::CLOSE => sys_close(a1),
        num::SENDFILE => sys_sendfile(a1, a2, a3, a4),
        num::GETPID => crate::process::getpid() as u64,
        num::GETTID => crate::process::gettid() as u64,
        num::GETUID | num::GETEUID => crate::process::getuid() as u64,
        num::GETGID | num::GETEGID => 0u64, // single-user kernel for now
        // Single-user kernel: accept setuid/setgid as no-ops (busybox drops privs).
        num::SETUID | num::SETGID => 0u64,
        num::RT_SIGACTION => sys_rt_sigaction(a1, a2, a3, a4),
        num::RT_SIGPROCMASK => sys_rt_sigprocmask(a1, a2, a3, a4),
        num::RT_SIGRETURN => sys_rt_sigreturn(),
        num::KILL => sys_kill(a1, a2),
        num::TKILL => sys_tkill(a1, a2),
        num::TGKILL => sys_tgkill(a1, a2, a3),
        // One supplementary group (0); busybox `id` uses this.
        num::GETGROUPS => sys_getgroups(a1, a2),
        num::GETPPID => {
            let pp = crate::process::getppid();
            if pp < 0 {
                0
            } else {
                pp as u64
            }
        }
        num::STAT => sys_stat(a1, a2),
        num::LSTAT => sys_lstat(a1, a2),
        num::FSTAT => sys_fstat(a1, a2),
        num::NEWFSTATAT => sys_newfstatat(a1, a2, a3, a4),
        num::LSEEK => sys_lseek(a1, a2, a3),
        num::OPENAT => sys_openat(a1, a2, a3, a4),
        num::MKDIR => sys_mkdir(a1, a2),
        num::MKDIRAT => sys_mkdirat(a1, a2, a3),
        num::RMDIR => sys_rmdir(a1),
        num::UNLINK => sys_unlink(a1),
        num::UNLINKAT => sys_unlinkat(a1, a2, a3),
        num::RENAME => sys_rename(a1, a2),
        num::RENAMEAT => sys_renameat(a1, a2, a3, a4),
        num::LINK => sys_link(a1, a2),
        num::SYMLINK => sys_symlink(a1, a2),
        num::READLINK => sys_readlink(a1, a2, a3),
        num::SYMLINKAT => sys_symlinkat(a1, a2, a3),
        num::READLINKAT => sys_readlinkat(a1, a2, a3, a4),
        num::STATX => sys_statx(a1, a2, a3, a4, a5),
        num::PIPE | num::PIPE2 => sys_pipe(a1),
        num::POLL => sys_poll(a1, a2, a3),
        num::PPOLL => sys_ppoll(a1, a2, a3, a4),
        num::SELECT => sys_select(a1, a2, a3, a4, a5),
        num::EPOLL_CREATE => sys_epoll_create(a1),
        num::EPOLL_CREATE1 => sys_epoll_create1(a1),
        num::EPOLL_CTL => sys_epoll_ctl(a1, a2, a3, a4),
        num::EPOLL_WAIT => sys_epoll_wait(a1, a2, a3, a4),
        num::DUP => sys_dup(a1),
        num::DUP2 => sys_dup2(a1, a2),
        num::ACCESS => sys_access(a1, a2),
        num::FACCESSAT => sys_faccessat(a1, a2, a3, a4),
        num::CHMOD => sys_chmod(a1, a2),
        num::FCHMODAT => sys_fchmodat(a1, a2, a3, a4),
        num::CHOWN => 0u64, // single-user; pretend success
        num::UMASK => sys_umask(a1),
        num::SYNC => 0u64, // write-through FS; nothing to flush
        num::NANOSLEEP => sys_nanosleep(a1, a2),
        num::UTIMENSAT => sys_utimensat(a1, a2, a3, a4),
        num::FUTIMESAT => sys_futimesat(a1, a2, a3),
        num::UTIMES => sys_utimes(a1, a2),
        num::FORK => sys_fork(),
        num::CLONE => sys_clone(a1, a2, a3, a4, a5),
        num::CLONE3 => sys_clone3(a1, a2),
        num::EXECVE => sys_execve(a1, a2, a3),
        num::EXECVEAT => sys_execveat(a1, a2, a3, a4, a5),
        num::WAIT4 => sys_wait4(a1, a2, a3),
        num::GETCWD => sys_getcwd(a1, a2),
        num::CHDIR => sys_chdir(a1),
        num::GETDENTS64 => sys_getdents64(a1, a2, a3),
        num::UNAME => sys_uname(a1),
        num::PRCTL => sys_prctl(a1, a2, a3, a4, a5),
        num::ARCH_PRCTL => sys_arch_prctl(a1, a2),
        num::BRK => sys_brk(a1),
        num::MMAP => {
            let a6 = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(last_user_r9)) };
            sys_mmap(a1, a2, a3, a4, a5, a6)
        }
        num::MPROTECT => sys_mprotect(a1, a2, a3),
        num::MUNMAP => sys_munmap(a1, a2),
        num::SET_TID_ADDRESS => sys_set_tid_address(a1),
        num::FUTEX => {
            let a6 = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(last_user_r9)) };
            sys_futex(a1, a2, a3, a4, a5, a6)
        }
        num::GETTIMEOFDAY => sys_gettimeofday(a1, a2),
        num::CLOCK_GETTIME => sys_clock_gettime(a1, a2),
        num::FCNTL => sys_fcntl(a1, a2, a3),
        num::INIT_MODULE => sys_init_module(a1, a2, a3),
        num::DELETE_MODULE => sys_delete_module(a1, a2),
        num::FINIT_MODULE => sys_finit_module(a1, a2, a3),
        num::PREAD64 => sys_pread64(a1, a2, a3, a4),
        num::SET_ROBUST_LIST => sys_set_robust_list(a1, a2),
        num::GETRANDOM => sys_getrandom(a1, a2, a3),
        num::PRLIMIT64 => sys_prlimit64(a1, a2, a3, a4),
        num::RSEQ => 0u64, // glibc probes; no restartable sequences yet
        num::EXIT => {
            let status = a1 as i32;
            finish_exit(status, false);
        }
        num::EXIT_GROUP => {
            let status = a1 as i32;
            finish_exit(status, true);
        }
        _ => {
            // Always log so musl/static binary bring-up is not blind.
            console::print("syscall: ENOSYS n=");
            console::write_u64(num);
            console::println(" (-38)");
            errno::neg(errno::ENOSYS)
        }
    };

    // After the syscall result is known: maybe enter a user signal handler
    // instead of returning to the interrupted rip.
    {
        use crate::process::signal_queue::DeliverResult;
        use crate::process::TrapFrame;
        let (rip, rsp, rflags) = unsafe {
            (
                core::ptr::read_volatile(core::ptr::addr_of!(last_user_rip)),
                core::ptr::read_volatile(core::ptr::addr_of!(last_user_rsp)),
                core::ptr::read_volatile(core::ptr::addr_of!(last_user_rflags)),
            )
        };
        let mut restore = TrapFrame::from_user_entry(rip, rsp, rflags, ret);
        // Preserve rax as the syscall return value for after the handler.
        restore.rax = ret;
        match crate::process::signal_queue::try_deliver_one(&restore) {
            DeliverResult::Handler(frame) => {
                crate::process::apply_tls();
                static mut SIG_FRAME: TrapFrame = TrapFrame::zero();
                unsafe {
                    SIG_FRAME = frame;
                    resume_user_trap(core::ptr::addr_of!(SIG_FRAME));
                }
            }
            DeliverResult::Fatal(sig) => {
                fatal_signal_exit(sig);
            }
            DeliverResult::None => {}
        }
    }

    // sysret path: put this process's TLS bases back in the CPU.
    crate::process::apply_tls();
    ret
}

/// Linux rt_sigreturn — restore context saved at signal-handler entry.
fn sys_rt_sigreturn() -> u64 {
    match crate::process::signal_queue::do_sigreturn() {
        Some(frame) => {
            crate::process::apply_tls();
            static mut RET_FRAME: crate::process::TrapFrame = crate::process::TrapFrame::zero();
            unsafe {
                RET_FRAME = frame;
                resume_user_trap(core::ptr::addr_of!(RET_FRAME));
            }
        }
        None => {
            // Bogus sigreturn — kill the process.
            fatal_signal_exit(crate::process::signal_queue::SIGKILL);
        }
    }
}

/// Linux `struct utsname` — six fields of 65 bytes each (incl. Linux domainname).
const UTS_LEN: usize = 65;
const UTS_NFIELD: usize = 6;
const UTS_SIZE: usize = UTS_LEN * UTS_NFIELD; // 390

fn put_uts_field(buf: &mut [u8], field: usize, s: &str) {
    let start = field * UTS_LEN;
    if start + UTS_LEN > buf.len() {
        return;
    }
    let slot = &mut buf[start..start + UTS_LEN];
    slot.fill(0);
    let n = core::cmp::min(s.len(), UTS_LEN - 1);
    slot[..n].copy_from_slice(&s.as_bytes()[..n]);
}

/// Linux uname(2) — fill user `struct utsname`.
fn sys_uname(buf_ptr: u64) -> u64 {
    if !user_ptr_ok(buf_ptr, UTS_SIZE as u64) {
        return errno::neg(errno::EFAULT);
    }
    let mut uts = [0u8; UTS_SIZE];
    put_uts_field(&mut uts, 0, "munux"); // sysname
    put_uts_field(&mut uts, 1, "munux"); // nodename
    put_uts_field(&mut uts, 2, "0.2.0"); // release
    put_uts_field(&mut uts, 3, "munux 0.2 x86_64"); // version
    put_uts_field(&mut uts, 4, "x86_64"); // machine
    put_uts_field(&mut uts, 5, ""); // domainname
    unsafe {
        core::ptr::copy_nonoverlapping(uts.as_ptr(), buf_ptr as *mut u8, UTS_SIZE);
    }
    0
}

/// Linux brk(2) — set or query the program break.
///
/// Syscall return value is always the resulting break address (new on success,
/// old if the request is invalid / OOM). `brk(0)` therefore returns the current
/// break (Linux rejects 0 as below `start_brk`).
fn sys_brk(new_brk: u64) -> u64 {
    crate::process::proc_brk(new_brk)
}

/// Linux mmap(2) — anon / file `MAP_PRIVATE` snapshot / file `MAP_SHARED` writeback.
fn sys_mmap(addr: u64, length: u64, prot: u64, flags: u64, fd: u64, offset: u64) -> u64 {
    match crate::process::proc_mmap(addr, length, prot, flags, fd, offset) {
        Ok(va) => va,
        Err(e) => errno::neg(e),
    }
}

/// Linux mprotect(2) — change page protections on an existing mapping.
fn sys_mprotect(addr: u64, length: u64, prot: u64) -> u64 {
    match crate::process::proc_mprotect(addr, length, prot) {
        Ok(()) => 0,
        Err(e) => errno::neg(e),
    }
}

/// Linux munmap(2) — unmap a whole region previously returned by mmap.
fn sys_munmap(addr: u64, length: u64) -> u64 {
    match crate::process::proc_munmap(addr, length) {
        Ok(()) => 0,
        Err(e) => errno::neg(e),
    }
}

/// Linux set_tid_address(2) — record clear_child_tid pointer; return tid.
///
/// Musl calls this during crt init. Clear-on-exit + futex wake on exit (Phase 6).
fn sys_set_tid_address(tidptr: u64) -> u64 {
    let _ = crate::process::with_current(|p| {
        p.clear_child_tid = tidptr;
    });
    crate::process::gettid() as u64
}

/// Convert signal helper status (0 or -errno) to syscall return.
fn sig_ret(r: i32) -> u64 {
    if r < 0 {
        r as u64
    } else {
        0
    }
}

fn sys_kill(pid: u64, sig: u64) -> u64 {
    sig_ret(crate::process::signal_queue::proc_kill(pid as i32, sig as u32))
}

fn sys_tkill(tid: u64, sig: u64) -> u64 {
    sig_ret(crate::process::signal_queue::proc_tkill(tid as i32, sig as u32))
}

fn sys_tgkill(tgid: u64, tid: u64, sig: u64) -> u64 {
    sig_ret(crate::process::signal_queue::proc_tgkill(
        tgid as i32,
        tid as i32,
        sig as u32,
    ))
}

/// Linux rt_sigprocmask(how, set, oldset, sigsetsize).
fn sys_rt_sigprocmask(how: u64, set: u64, oldset: u64, sigsetsize: u64) -> u64 {
    // Accept 8 or 16 byte sets; we only use low 64 bits.
    if sigsetsize != 0 && sigsetsize != 8 && sigsetsize != 16 {
        return errno::neg(errno::EINVAL);
    }
    let new_set = if set != 0 {
        if !user_ptr_ok(set, 8) {
            return errno::neg(errno::EFAULT);
        }
        Some(unsafe { core::ptr::read_volatile(set as *const u64) })
    } else {
        None
    };
    let old = crate::process::signal_queue::proc_sigprocmask(how as u32, new_set);
    if oldset != 0 {
        if !user_ptr_ok(oldset, 8) {
            return errno::neg(errno::EFAULT);
        }
        unsafe {
            core::ptr::write_volatile(oldset as *mut u64, old);
            if sigsetsize >= 16 {
                core::ptr::write_volatile((oldset + 8) as *mut u64, 0);
            }
        }
    }
    0
}

/// Linux rt_sigaction(sig, act, oldact, sigsetsize).
///
/// Reads only `sa_handler` (first 8 bytes of user `struct sigaction`).
fn sys_rt_sigaction(sig: u64, act: u64, oldact: u64, _sigsetsize: u64) -> u64 {
    let sig = sig as u32;
    if sig == 0 || sig as usize >= crate::process::pcb::MAX_SIGNALS {
        return errno::neg(errno::EINVAL);
    }
    if sig == crate::process::signal_queue::SIGKILL || sig == crate::process::signal_queue::SIGSTOP
    {
        return errno::neg(errno::EINVAL);
    }

    let old_h = crate::process::with_current(|p| p.sig_handlers[sig as usize]).unwrap_or(0);

    if oldact != 0 {
        if !user_ptr_ok(oldact, 8) {
            return errno::neg(errno::EFAULT);
        }
        unsafe {
            core::ptr::write_volatile(oldact as *mut u64, old_h as u64);
        }
    }

    if act != 0 {
        if !user_ptr_ok(act, 8) {
            return errno::neg(errno::EFAULT);
        }
        let handler = unsafe { core::ptr::read_volatile(act as *const u64) } as usize;
        let r = crate::process::signal_queue::proc_signal(sig, handler);
        if r == usize::MAX {
            return errno::neg(errno::EINVAL);
        }
    }
    0
}

// Linux futex ops (include/uapi/linux/futex.h)
const FUTEX_WAIT: u32 = 0;
const FUTEX_WAKE: u32 = 1;
const FUTEX_REQUEUE: u32 = 3;
const FUTEX_CMP_REQUEUE: u32 = 4;
const FUTEX_WAIT_BITSET: u32 = 9;
const FUTEX_WAKE_BITSET: u32 = 10;
const FUTEX_PRIVATE_FLAG: u32 = 128;
const FUTEX_CLOCK_REALTIME: u32 = 256;
const FUTEX_CMD_MASK: u32 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

/// Parse optional relative `struct timespec` at `timeout_ptr`.
/// Returns `Some(deadline_tick)` or `None` for infinite wait.
/// `Err` → already-negated errno for the syscall.
fn futex_deadline(timeout_ptr: u64) -> Result<Option<u64>, u64> {
    if timeout_ptr == 0 {
        return Ok(None);
    }
    if !user_ptr_ok(timeout_ptr, 16) {
        return Err(errno::neg(errno::EFAULT));
    }
    let sec = unsafe { core::ptr::read_volatile(timeout_ptr as *const i64) };
    let nsec = unsafe { core::ptr::read_volatile((timeout_ptr + 8) as *const i64) };
    if sec < 0 || nsec < 0 || nsec >= 1_000_000_000 {
        return Err(errno::neg(errno::EINVAL));
    }
    let total_ns = (sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(nsec as u64);
    // PIT 100 Hz → 10 ms ticks; at least 1 tick if any positive wait.
    let ticks_needed = if total_ns == 0 {
        0
    } else {
        ((total_ns + 9_999_999) / 10_000_000).max(1)
    };
    let start = crate::interrupts::ticks();
    Ok(Some(start.wrapping_add(ticks_needed)))
}

/// Deadline is absolute tick count; expired when `ticks() >= deadline`.
fn futex_deadline_reached(deadline: Option<u64>) -> bool {
    match deadline {
        None => false,
        Some(d) => crate::interrupts::ticks() >= d,
    }
}

/// Shared wait body for FUTEX_WAIT / FUTEX_WAIT_BITSET.
fn futex_do_wait(uaddr: u64, expected: i32, private: bool, timeout_ptr: u64) -> u64 {
    let deadline = match futex_deadline(timeout_ptr) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let me = crate::process::gettid();
    match crate::process::futex::begin_wait(uaddr, expected, private) {
        Err(e) => return e as u64,
        Ok(()) => {}
    }

    // Cooperatively run **Ready children** only (not arbitrary system tasks —
    // picking shell/kinit via take_ready(-1) nest-corrupts and kills the waiter).
    let mut idle_rounds: u32 = 0;
    loop {
        if !crate::process::futex::still_waiting(me) {
            break;
        }
        let cur = unsafe { core::ptr::read_volatile(uaddr as *const i32) };
        if cur != expected {
            crate::process::futex::cancel_wait(me);
            break;
        }
        if futex_deadline_reached(deadline) {
            crate::process::futex::cancel_wait(me);
            let _ = crate::process::with_current(|p| {
                if p.state == crate::process::ProcessState::Sleeping {
                    p.state = crate::process::ProcessState::Running;
                }
            });
            return errno::neg(errno::ETIMEDOUT);
        }

        // Prefer any Ready task that is our child (thread or process).
        if let Some(frame) = take_ready_child() {
            run_user_frame(frame);
            crate::process::futex::ensure_running_if_current();
            idle_rounds = 0;
        } else {
            // Nested futex wait (shell → app → thread): if we spin forever here,
            // the outer task never unlocks/wakes us → deadlock. Return a
            // spurious wake so userspace rechecks; outer nest resumes.
            let nest = unsafe {
                extern "C" {
                    fn get_enter_nest_depth() -> u64;
                }
                get_enter_nest_depth()
            };
            if nest >= 2 && deadline.is_none() {
                crate::process::futex::cancel_wait(me);
                crate::process::futex::ensure_running_if_current();
                return 0;
            }

            idle_rounds = idle_rounds.saturating_add(1);
            unsafe {
                core::arch::asm!("sti; pause", options(nostack, nomem));
            }
            // Top-level infinite wait, nothing to run: soft-fail.
            if deadline.is_none() && idle_rounds > 5_000_000 {
                crate::process::futex::cancel_wait(me);
                let _ = crate::process::with_current(|p| {
                    if p.state == crate::process::ProcessState::Sleeping {
                        p.state = crate::process::ProcessState::Running;
                    }
                });
                return errno::neg(errno::EAGAIN);
            }
        }
    }
    if crate::process::futex::still_waiting(me) {
        crate::process::futex::cancel_wait(me);
    }
    crate::process::futex::ensure_running_if_current();
    0
}

/// Like `sched::take_ready` but **only** Ready tasks with `parent == current`.
fn take_ready_child() -> Option<crate::process::UserFrame> {
    let parent = crate::process::gettid();
    let mut child = -1i32;
    crate::process::table::for_each_process(|_i, p| {
        if child >= 0 {
            return;
        }
        if p.used && p.state == crate::process::ProcessState::Ready && p.parent == parent {
            child = p.pid;
        }
    });
    if child > 0 {
        crate::process::sched::take_ready(child)
    } else {
        None
    }
}

/// Public helper for pipes: run one Ready child if any, else pause.
pub fn try_run_ready_child() {
    if let Some(frame) = take_ready_child() {
        run_user_frame(frame);
    } else {
        unsafe {
            core::arch::asm!("sti; pause", options(nostack, nomem));
        }
    }
}

/// Linux futex(2) — wait/wake/requeue + bitset aliases; relative timeout on wait.
///
/// Args: uaddr, op, val, timeout|nr_requeue, uaddr2, val3.
fn sys_futex(uaddr: u64, op: u64, val: u64, a4: u64, uaddr2: u64, val3: u64) -> u64 {
    if !user_ptr_ok(uaddr, 4) || (uaddr & 3) != 0 {
        return errno::neg(errno::EFAULT);
    }
    let op = op as u32;
    let cmd = op & FUTEX_CMD_MASK;
    let private = (op & FUTEX_PRIVATE_FLAG) != 0;

    match cmd {
        FUTEX_WAIT => futex_do_wait(uaddr, val as i32, private, a4),
        FUTEX_WAIT_BITSET => {
            // Absolute timeout + bitset in Linux; we accept relative timespec at a4
            // when provided and ignore bitset except requiring non-zero.
            let bitset = val3 as u32;
            if bitset == 0 {
                return errno::neg(errno::EINVAL);
            }
            let _ = bitset; // MATCH_ANY or any non-zero: treat as plain wait
            futex_do_wait(uaddr, val as i32, private, a4)
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => {
            if cmd == FUTEX_WAKE_BITSET {
                let bitset = val3 as u32;
                if bitset == 0 {
                    return errno::neg(errno::EINVAL);
                }
            }
            let n = if val > u32::MAX as u64 {
                u32::MAX
            } else {
                val as u32
            };
            crate::process::futex::wake(uaddr, n, private) as u64
        }
        FUTEX_REQUEUE | FUTEX_CMP_REQUEUE => {
            if !user_ptr_ok(uaddr2, 4) || (uaddr2 & 3) != 0 {
                return errno::neg(errno::EFAULT);
            }
            if cmd == FUTEX_CMP_REQUEUE {
                let cur = unsafe { core::ptr::read_volatile(uaddr as *const i32) };
                if cur != val3 as i32 {
                    return errno::neg(errno::EAGAIN);
                }
            }
            // Linux: val = nr_wake, a4 (timeout slot) = nr_requeue (not a pointer).
            let nr_wake = if val > u32::MAX as u64 {
                u32::MAX
            } else {
                val as u32
            };
            let nr_requeue = if a4 > u32::MAX as u64 {
                u32::MAX
            } else {
                a4 as u32
            };
            crate::process::futex::requeue(uaddr, uaddr2, nr_wake, nr_requeue, private) as u64
        }
        _ => errno::neg(errno::ENOSYS),
    }
}

/// Common exit path: single-thread `exit` or whole-group `exit_group`.
fn finish_exit(status: i32, group: bool) -> ! {
    // Capture how this task was entered *before* we switch away.
    let via_nest = crate::process::with_current(|p| p.entered_via_nest).unwrap_or(true);
    crate::process::clear_tls();
    if group {
        crate::process::exit_group(status);
    } else {
        crate::process::exit_user(status);
    }
    // Current is now parent (or init).
    crate::process::apply_tls();
    if via_nest {
        extern "C" {
            fn get_enter_nest_depth() -> u64;
        }
        if unsafe { get_enter_nest_depth() } > 0 {
            unsafe {
                return_from_user();
            }
        }
    }
    resume_current_from_trap();
}

/// Nest-safe process exit from signal/TTY path (e.g. Ctrl-C mid `read`).
///
/// Status is encoded like a fatal signal: `128 + sig` (low 8 bits).
pub fn fatal_signal_exit(sig: u32) -> ! {
    let status = (128 + (sig as i32)) & 0xff;
    finish_exit(status, true);
}

// Linux clockid_t (subset)
const CLOCK_REALTIME: u64 = 0;
const CLOCK_MONOTONIC: u64 = 1;
const CLOCK_MONOTONIC_RAW: u64 = 4;
const CLOCK_REALTIME_COARSE: u64 = 5;
const CLOCK_MONOTONIC_COARSE: u64 = 6;
const CLOCK_BOOTTIME: u64 = 7;

/// Fixed REALTIME origin at boot (no CMOS RTC yet). Ext2 write uses a similar base.
const REALTIME_EPOCH_BASE_SEC: u64 = 1_700_000_000;

/// (sec, nsec) for wall clock = epoch base + uptime.
fn wall_time() -> (u64, u64) {
    let ns = crate::interrupts::timer::uptime_ns();
    let sec = REALTIME_EPOCH_BASE_SEC.saturating_add(ns / 1_000_000_000);
    let nsec = ns % 1_000_000_000;
    (sec, nsec)
}

/// (sec, nsec) monotonic since boot.
fn mono_time() -> (u64, u64) {
    let ns = crate::interrupts::timer::uptime_ns();
    (ns / 1_000_000_000, ns % 1_000_000_000)
}

/// Linux clock_gettime(2) — fill user `struct timespec { i64 tv_sec; i64 tv_nsec; }`.
fn sys_clock_gettime(clkid: u64, tp: u64) -> u64 {
    if !user_ptr_ok(tp, 16) {
        return errno::neg(errno::EFAULT);
    }
    let (sec, nsec) = match clkid {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE => wall_time(),
        CLOCK_MONOTONIC | CLOCK_MONOTONIC_RAW | CLOCK_MONOTONIC_COARSE | CLOCK_BOOTTIME => {
            mono_time()
        }
        _ => return errno::neg(errno::EINVAL),
    };
    unsafe {
        core::ptr::write_volatile(tp as *mut i64, sec as i64);
        core::ptr::write_volatile((tp + 8) as *mut i64, nsec as i64);
    }
    0
}

/// Linux gettimeofday(2) — fill `struct timeval { i64 tv_sec; i64 tv_usec; }`.
/// `tz` is ignored (Linux also treats it as obsolete).
fn sys_gettimeofday(tv: u64, _tz: u64) -> u64 {
    if tv != 0 {
        if !user_ptr_ok(tv, 16) {
            return errno::neg(errno::EFAULT);
        }
        let (sec, nsec) = wall_time();
        let usec = nsec / 1000;
        unsafe {
            core::ptr::write_volatile(tv as *mut i64, sec as i64);
            core::ptr::write_volatile((tv + 8) as *mut i64, usec as i64);
        }
    }
    0
}

/// Linux fcntl(2) — minimal support for musl `opendir` (F_SETFD CLOEXEC) etc.
fn sys_fcntl(fd: u64, cmd: u64, arg: u64) -> u64 {
    match crate::fd::sys_fcntl(fd, cmd, arg) {
        Ok(v) => v,
        Err(e) => map_fd_err(e),
    }
}

// Linux arch/x86/include/uapi/asm/prctl.h
const ARCH_SET_GS: u64 = 0x1001;
const ARCH_SET_FS: u64 = 0x1002;
const ARCH_GET_FS: u64 = 0x1003;
const ARCH_GET_GS: u64 = 0x1004;

/// Linux arch_prctl(2) — set/get FS/GS base for TLS.
///
/// `arg` for SET is the **base address value** (not a pointer to it).
/// For GET, `arg` is a user pointer where the base is stored.
fn sys_arch_prctl(code: u64, arg: u64) -> u64 {
    match code {
        ARCH_SET_FS => {
            // Canonical user address (or 0 to clear). Musl may point slightly
            // outside a single mapped page; allow full lower half user VA.
            if arg != 0 && (arg < 0x1000 || arg >= 0x0000_8000_0000_0000) {
                return errno::neg(errno::EFAULT);
            }
            // Only update PCB here — dispatch restores MSRs for sysret.
            // Avoid leaving user FS loaded during the rest of the syscall path.
            let _ = crate::process::with_current(|p| {
                p.fs_base = arg;
            });
            0
        }
        ARCH_SET_GS => {
            if arg != 0 && (arg < 0x1000 || arg >= 0x0000_8000_0000_0000) {
                return errno::neg(errno::EFAULT);
            }
            let _ = crate::process::with_current(|p| {
                p.gs_base = arg;
            });
            0
        }
        ARCH_GET_FS => {
            if !user_ptr_ok(arg, 8) {
                return errno::neg(errno::EFAULT);
            }
            let v = crate::process::get_fs_base_saved();
            unsafe {
                core::ptr::write_volatile(arg as *mut u64, v);
            }
            0
        }
        ARCH_GET_GS => {
            if !user_ptr_ok(arg, 8) {
                return errno::neg(errno::EFAULT);
            }
            let v = crate::process::get_gs_base_saved();
            unsafe {
                core::ptr::write_volatile(arg as *mut u64, v);
            }
            0
        }
        _ => {
            console::print("syscall: arch_prctl unknown code=");
            console::write_hex64(code);
            console::println("");
            errno::neg(errno::EINVAL)
        }
    }
}

// Linux include/uapi/linux/prctl.h (subset userspace actually calls).
const PR_SET_PDEATHSIG: u64 = 1;
const PR_GET_PDEATHSIG: u64 = 2;
const PR_GET_DUMPABLE: u64 = 3;
const PR_SET_DUMPABLE: u64 = 4;
const PR_SET_NAME: u64 = 15;
const PR_GET_NAME: u64 = 16;
const PR_GET_SECCOMP: u64 = 21;
const PR_SET_SECCOMP: u64 = 22;
const PR_CAPBSET_READ: u64 = 23;
const PR_SET_NO_NEW_PRIVS: u64 = 38;
const PR_GET_NO_NEW_PRIVS: u64 = 39;
const PR_GET_TID_ADDRESS: u64 = 40;
const PR_SET_PTRACER: u64 = 0x5961_6d61; // "Yama"

/// Linux prctl(2) — process control used by musl, sandboxes, and tooling.
fn sys_prctl(option: u64, arg2: u64, _arg3: u64, _arg4: u64, _arg5: u64) -> u64 {
    match option {
        PR_SET_PDEATHSIG => {
            let sig = arg2 as u32;
            if sig != 0 && (sig >= crate::process::pcb::MAX_SIGNALS as u32) {
                return errno::neg(errno::EINVAL);
            }
            let _ = crate::process::with_current(|p| {
                p.pdeathsig = sig;
            });
            0
        }
        PR_GET_PDEATHSIG => {
            if !user_ptr_ok(arg2, 4) {
                return errno::neg(errno::EFAULT);
            }
            let sig = crate::process::with_current(|p| p.pdeathsig).unwrap_or(0);
            unsafe {
                core::ptr::write_volatile(arg2 as *mut i32, sig as i32);
            }
            0
        }
        PR_GET_DUMPABLE => crate::process::with_current(|p| p.dumpable as u64).unwrap_or(1),
        PR_SET_DUMPABLE => {
            if arg2 > 2 {
                return errno::neg(errno::EINVAL);
            }
            let _ = crate::process::with_current(|p| {
                p.dumpable = arg2 as u8;
            });
            0
        }
        PR_SET_NAME => {
            if arg2 == 0 || !user_ptr_ok(arg2, 1) {
                return errno::neg(errno::EFAULT);
            }
            let mut buf = [0u8; 16];
            for i in 0..15 {
                if !user_ptr_ok(arg2 + i as u64, 1) {
                    break;
                }
                let b = unsafe { core::ptr::read_volatile((arg2 as usize + i) as *const u8) };
                if b == 0 {
                    break;
                }
                buf[i] = b;
            }
            let len = buf.iter().position(|&c| c == 0).unwrap_or(15);
            let s = core::str::from_utf8(&buf[..len]).unwrap_or("?");
            let _ = crate::process::with_current(|p| p.set_name(s));
            0
        }
        PR_GET_NAME => {
            if !user_ptr_ok(arg2, 16) {
                return errno::neg(errno::EFAULT);
            }
            let name = crate::process::with_current(|p| p.name).unwrap_or([0; 16]);
            unsafe {
                core::ptr::copy_nonoverlapping(name.as_ptr(), arg2 as *mut u8, 16);
            }
            0
        }
        PR_GET_SECCOMP => 0, // SECCOMP_MODE_DISABLED
        PR_SET_SECCOMP => errno::neg(errno::EINVAL),
        PR_CAPBSET_READ => {
            // Single-user: every valid cap is in the bounding set.
            if arg2 >= 64 {
                errno::neg(errno::EINVAL)
            } else {
                1
            }
        }
        PR_SET_NO_NEW_PRIVS => {
            if arg2 != 1 {
                return errno::neg(errno::EINVAL);
            }
            let _ = crate::process::with_current(|p| {
                p.no_new_privs = true;
            });
            0
        }
        PR_GET_NO_NEW_PRIVS => {
            if crate::process::with_current(|p| p.no_new_privs).unwrap_or(false) {
                1
            } else {
                0
            }
        }
        PR_GET_TID_ADDRESS => {
            if !user_ptr_ok(arg2, 8) {
                return errno::neg(errno::EFAULT);
            }
            let addr = crate::process::with_current(|p| p.clear_child_tid).unwrap_or(0);
            unsafe {
                core::ptr::write_volatile(arg2 as *mut u64, addr);
            }
            0
        }
        PR_SET_PTRACER => 0, // no ptrace yet; sandboxes probe this
        _ => {
            console::print("syscall: prctl unknown option=");
            console::write_u64(option);
            console::println("");
            errno::neg(errno::EINVAL)
        }
    }
}

fn user_ptr_ok(buf: u64, len: u64) -> bool {
    if len > 0x10000 {
        return false;
    }
    if len == 0 {
        return true;
    }
    let end = buf.saturating_add(len);
    // Demo blob, classic ELF load (0x400000+), brk heap, mmap arena, stacks
    (buf >= DEMO_CODE && end <= DEMO_STACK_TOP + 0x1000)
        || (buf >= 0x400000 && end <= 0x800000)
        // brk heap + mmap arena (musl stdio buffers often live here)
        || (buf >= 0x1000 && end <= 0x0000_0000_7000_0000)
        || (buf >= 0x0000_0000_7000_0000 && end <= 0x0000_0000_8000_0000)
        || (buf >= 0x0000_0000_6F00_0000 && end <= 0x0000_0000_7000_0000)
}

fn sys_write(fd: u64, buf: u64, len: u64) -> u64 {
    let len = len.min(4096);
    if len == 0 {
        return 0;
    }
    if !user_ptr_ok(buf, len) {
        return errno::neg(errno::EFAULT);
    }
    let slice = unsafe { core::slice::from_raw_parts(buf as *const u8, len as usize) };
    match fd::sys_write_slice(fd, slice) {
        Ok(n) => n as u64,
        Err(e) => map_fd_err(e),
    }
}

/// Kernel buffer for sendfile (keep off the nest stack).
static mut SENDFILE_BUF: [u8; 4096] = [0; 4096];

/// Linux sendfile(out_fd, in_fd, *offset, count).
///
/// Copies up to `count` bytes from `in_fd` to `out_fd`. If `offset` is non-null,
/// uses/updates that file position and does **not** change `in_fd`'s offset
/// (Linux semantics). If null, advances `in_fd`'s offset.
///
/// Implemented as a read→write loop (no zero-copy); enough for BusyBox `cat`.
fn sys_sendfile(out_fd: u64, in_fd: u64, offset_ptr: u64, count: u64) -> u64 {
    if count == 0 {
        return 0;
    }
    // Cap one sendfile call (cat often passes full file size; loop until done).
    let mut remaining = count.min(16 * 1024 * 1024); // 16 MiB safety cap
    let use_user_off = offset_ptr != 0;
    if use_user_off && !user_ptr_ok(offset_ptr, 8) {
        return errno::neg(errno::EFAULT);
    }

    let mut pos: u64 = if use_user_off {
        unsafe { core::ptr::read_volatile(offset_ptr as *const u64) }
    } else {
        match fd::sys_fd_offset(in_fd) {
            Ok(o) => o,
            Err(e) => return map_fd_err(e),
        }
    };

    let buf = unsafe { &mut *core::ptr::addr_of_mut!(SENDFILE_BUF) };
    let mut total: u64 = 0;

    while remaining > 0 {
        let chunk = (remaining as usize).min(buf.len());
        // Read at `pos` without permanently moving in_fd if user offset is set.
        let n_read = if use_user_off {
            match fd::sys_read_at(in_fd, pos, &mut buf[..chunk]) {
                Ok(n) => n,
                Err(e) => {
                    return if total == 0 {
                        map_fd_err(e)
                    } else {
                        total
                    };
                }
            }
        } else {
            match fd::sys_read_into(in_fd, &mut buf[..chunk]) {
                Ok(n) => n,
                Err(e) => {
                    return if total == 0 {
                        map_fd_err(e)
                    } else {
                        total
                    };
                }
            }
        };
        if n_read == 0 {
            break; // EOF
        }
        // Write all of n_read to out (console write may be partial-ish; retry).
        let mut written = 0usize;
        while written < n_read {
            match fd::sys_write_slice(out_fd, &buf[written..n_read]) {
                Ok(0) => break,
                Ok(w) => written += w,
                Err(e) => {
                    return if total == 0 && written == 0 {
                        map_fd_err(e)
                    } else {
                        total.saturating_add(written as u64)
                    };
                }
            }
        }
        if written == 0 {
            break;
        }
        total = total.saturating_add(written as u64);
        pos = pos.saturating_add(written as u64);
        remaining = remaining.saturating_sub(written as u64);
        // If write was short vs read, still advance by what was written.
        if written < n_read {
            break;
        }
    }

    if use_user_off {
        unsafe {
            core::ptr::write_volatile(offset_ptr as *mut u64, pos);
        }
    }
    // When offset_ptr is null, sys_read_into already advanced in_fd.offset.
    total
}

/// Linux `struct iovec` — two u64 fields on x86_64.
const IOV_SIZE: u64 = 16;
const IOV_MAX: u64 = 16;

/// Linux writev(2) — used by musl stdio (`printf`).
fn sys_writev(fd: u64, iov_ptr: u64, iovcnt: u64) -> u64 {
    if iovcnt == 0 {
        return 0;
    }
    if iovcnt > IOV_MAX {
        return errno::neg(errno::EINVAL);
    }
    let bytes = iovcnt.saturating_mul(IOV_SIZE);
    if !user_ptr_ok(iov_ptr, bytes) {
        return errno::neg(errno::EFAULT);
    }
    let mut total: u64 = 0;
    for i in 0..iovcnt {
        let base = unsafe {
            core::ptr::read_volatile((iov_ptr + i * IOV_SIZE) as *const u64)
        };
        let len = unsafe {
            core::ptr::read_volatile((iov_ptr + i * IOV_SIZE + 8) as *const u64)
        };
        if len == 0 {
            continue;
        }
        let n = sys_write(fd, base, len);
        // Negative errno?
        if (n as i64) < 0 {
            return if total == 0 { n } else { total };
        }
        total = total.saturating_add(n);
        if n < len {
            break; // short write
        }
    }
    total
}

/// Linux readv(2) — used by musl stdio (`fread` / file buffering).
///
/// Scatter-read: fill iov[0], then iov[1], … until request done or EOF.
/// Same iovec layout as writev.
fn sys_readv(fd: u64, iov_ptr: u64, iovcnt: u64) -> u64 {
    if iovcnt == 0 {
        return 0;
    }
    if iovcnt > IOV_MAX {
        return errno::neg(errno::EINVAL);
    }
    let bytes = iovcnt.saturating_mul(IOV_SIZE);
    if !user_ptr_ok(iov_ptr, bytes) {
        return errno::neg(errno::EFAULT);
    }
    let mut total: u64 = 0;
    for i in 0..iovcnt {
        let base = unsafe {
            core::ptr::read_volatile((iov_ptr + i * IOV_SIZE) as *const u64)
        };
        let len = unsafe {
            core::ptr::read_volatile((iov_ptr + i * IOV_SIZE + 8) as *const u64)
        };
        if len == 0 {
            continue;
        }
        // May take multiple sys_read chunks if len > 4096 (sys_read cap).
        let mut filled: u64 = 0;
        while filled < len {
            let n = sys_read(fd, base + filled, len - filled);
            if (n as i64) < 0 {
                return if total == 0 { n } else { total };
            }
            if n == 0 {
                // EOF
                return total.saturating_add(filled);
            }
            filled = filled.saturating_add(n);
        }
        total = total.saturating_add(filled);
    }
    total
}

/// Linux ioctl(2) — stub: console is not a TTY; musl accepts ENOTTY for TIOCGWINSZ.
fn sys_ioctl(_fd: u64, _cmd: u64, _arg: u64) -> u64 {
    errno::neg(errno::ENOTTY)
}

fn sys_read(fd: u64, buf: u64, len: u64) -> u64 {
    let len = len.min(4096) as usize;
    if len == 0 {
        return 0;
    }
    if !user_ptr_ok(buf, len as u64) {
        return errno::neg(errno::EFAULT);
    }
    let mut tmp = [0u8; 4096];
    let n = match fd::sys_read_into(fd, &mut tmp[..len]) {
        Ok(n) => n,
        Err(e) => return map_fd_err(e),
    };
    unsafe {
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), buf as *mut u8, n);
    }
    n as u64
}

fn sys_close(fd: u64) -> u64 {
    match fd::sys_close(fd) {
        Ok(()) => 0,
        Err(e) => map_fd_err(e),
    }
}

/// Linux pread64(fd, buf, count, offset) — read without moving the fd offset.
fn sys_pread64(fd: u64, buf: u64, count: u64, offset: u64) -> u64 {
    let len = count.min(4096) as usize;
    if len == 0 {
        return 0;
    }
    if !user_ptr_ok(buf, len as u64) {
        return errno::neg(errno::EFAULT);
    }
    let mut tmp = [0u8; 4096];
    match crate::fd::sys_read_at(fd, offset, &mut tmp[..len]) {
        Ok(n) => {
            unsafe {
                core::ptr::copy_nonoverlapping(tmp.as_ptr(), buf as *mut u8, n);
            }
            n as u64
        }
        Err(e) => map_fd_err(e),
    }
}

/// Linux set_robust_list — glibc always calls this; no robust futex yet.
fn sys_set_robust_list(_head: u64, _len: u64) -> u64 {
    0
}

/// Linux prlimit64 — glibc queries stack/nofile; report unlimited.
fn sys_prlimit64(_pid: u64, _resource: u64, _new: u64, old: u64) -> u64 {
    if old != 0 {
        if !user_ptr_ok(old, 16) {
            return errno::neg(errno::EFAULT);
        }
        unsafe {
            core::ptr::write_volatile(old as *mut u64, u64::MAX);
            core::ptr::write_volatile((old + 8) as *mut u64, u64::MAX);
        }
    }
    0
}

/// Linux getrandom(buf, buflen, flags) — not crypto; timer-mixed bytes for ld.so/TLS.
fn sys_getrandom(buf: u64, buflen: u64, _flags: u64) -> u64 {
    let len = buflen.min(256);
    if len == 0 {
        return 0;
    }
    if !user_ptr_ok(buf, len) {
        return errno::neg(errno::EFAULT);
    }
    let mut seed = crate::interrupts::timer::uptime_ns();
    for i in 0..len as usize {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        unsafe {
            core::ptr::write_volatile((buf as *mut u8).add(i), (seed >> 33) as u8);
        }
    }
    len
}

/// Linux open(path, flags, mode) — mode ignored for creat defaults in fd layer.
fn sys_open(path_ptr: u64, flags: u64, _mode: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    match fd::sys_open_path(path, flags) {
        Ok(fd) => fd as u64,
        Err(e) => map_fd_err(e),
    }
}

/// Linux openat(dirfd, path, flags, mode) — AT_FDCWD (-100) or absolute path only.
fn sys_openat(dirfd: u64, path_ptr: u64, flags: u64, mode: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    let _ = mode;
    if path.starts_with('/') || dirfd as i32 == AT_FDCWD {
        return match fd::sys_open_path(path, flags) {
            Ok(fd) => fd as u64,
            Err(e) => map_fd_err(e),
        };
    }
    // Relative to an open directory fd (ld.so: openat(/lib64, "libc.so.6")).
    let dino = match crate::fd::sys_fd_inode(dirfd) {
        Ok(i) => i,
        Err(crate::fd::FdError::BadFd) => return errno::neg(errno::EBADF),
        Err(_) => return errno::neg(errno::ENOTDIR),
    };
    match crate::fs::vcore::vfs_open_rel(dino, path, flags as u32) {
        Ok(data) => match crate::fd::sys_install_file(data) {
            Ok(fd) => fd as u64,
            Err(e) => map_fd_err(e),
        },
        Err(e) => map_vfs_stat_err(e),
    }
}

/// Linux x86_64 `struct stat` size (arch/x86/include/uapi/asm/stat.h).
const STAT_SIZE: usize = 144;

fn map_vfs_stat_err(e: crate::fs::vcore::VfsError) -> u64 {
    use crate::fs::vcore::VfsError;
    match e {
        VfsError::NoEnt => errno::neg(errno::ENOENT),
        VfsError::NotDir => errno::neg(errno::ENOTDIR),
        VfsError::Loop => errno::neg(errno::ELOOP),
        VfsError::IsDir => errno::neg(errno::EISDIR),
        VfsError::Inval => errno::neg(errno::EINVAL),
        _ => errno::neg(errno::ENOENT),
    }
}

/// Fill a Linux x86_64 `struct stat` at `stat_buf` from VFS metadata.
fn fill_linux_stat(stat_buf: u64, st: &crate::fs::vcore::VfsStat) -> u64 {
    if !user_ptr_ok(stat_buf, STAT_SIZE as u64) {
        return errno::neg(errno::EFAULT);
    }
    unsafe {
        core::ptr::write_bytes(stat_buf as *mut u8, 0, STAT_SIZE);
    }
    let blksize = if st.blksize == 0 { 1024 } else { st.blksize };
    let p = stat_buf as *mut u8;
    write_u64_le(p, 0, 0xdead); // st_dev
    write_u64_le(p, 8, st.ino);
    write_u64_le(p, 16, st.nlink as u64);
    write_u32_le(p, 24, st.mode as u32);
    write_u32_le(p, 28, st.uid as u32);
    write_u32_le(p, 32, st.gid as u32);
    write_u32_le(p, 36, 0);
    write_u64_le(p, 40, st.rdev as u64);
    write_u64_le(p, 48, st.size);
    write_u64_le(p, 56, blksize as u64);
    write_u64_le(p, 64, st.blocks_512);
    write_u64_le(p, 72, st.atime as u64);
    write_u64_le(p, 80, 0);
    write_u64_le(p, 88, st.mtime as u64);
    write_u64_le(p, 96, 0);
    write_u64_le(p, 104, st.ctime as u64);
    write_u64_le(p, 112, 0);
    0
}

fn write_u64_le(base: *mut u8, off: usize, v: u64) {
    unsafe {
        core::ptr::copy_nonoverlapping(v.to_le_bytes().as_ptr(), base.add(off), 8);
    }
}
fn write_u32_le(base: *mut u8, off: usize, v: u32) {
    unsafe {
        core::ptr::copy_nonoverlapping(v.to_le_bytes().as_ptr(), base.add(off), 4);
    }
}

fn resolve_user_ino(path: &str, follow: bool) -> Result<u32, u64> {
    if !crate::fs::is_ready() {
        return Err(errno::neg(errno::ENOENT));
    }
    let cwd = crate::fs::path::cwd_inode();
    let r = if follow {
        crate::fs::ext2::resolve_path(cwd, path)
    } else {
        crate::fs::ext2::resolve_lpath(cwd, path)
    };
    match r {
        Ok(i) => Ok(i),
        Err("too many symlinks") => Err(errno::neg(errno::ELOOP)),
        Err("not a directory") => Err(errno::neg(errno::ENOTDIR)),
        Err(_) => Err(errno::neg(errno::ENOENT)),
    }
}

fn stat_path(path: &str, stat_buf: u64, follow: bool) -> u64 {
    match crate::fs::vcore::vfs_stat(path, follow) {
        Ok(st) => fill_linux_stat(stat_buf, &st),
        Err(e) => map_vfs_stat_err(e),
    }
}

/// Linux stat(path, buf) — follows last symlink.
fn sys_stat(path_ptr: u64, stat_buf: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    stat_path(path, stat_buf, true)
}

/// Linux lstat(path, buf) — does not follow last symlink.
fn sys_lstat(path_ptr: u64, stat_buf: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    stat_path(path, stat_buf, false)
}

/// Linux fstat(fd, buf).
fn sys_fstat(fd: u64, stat_buf: u64) -> u64 {
    match fd::sys_fd_stat(fd) {
        Ok(st) => fill_linux_stat(stat_buf, &st),
        Err(e) => map_fd_err(e),
    }
}

/// Linux newfstatat(dirfd, path, buf, flags) — AT_FDCWD / absolute only.
fn sys_newfstatat(dirfd: u64, path_ptr: u64, stat_buf: u64, flags: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
    // Empty path + AT_EMPTY_PATH not supported; require a path.
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    stat_path(path, stat_buf, flags & AT_SYMLINK_NOFOLLOW == 0)
}

/// Linux lseek(fd, offset, whence).
fn sys_lseek(fd: u64, offset: u64, whence: u64) -> u64 {
    match fd::sys_lseek(fd, offset as i64, whence) {
        Ok(off) => off,
        Err(e) => map_fd_err(e),
    }
}

/// Linux getgroups(size, list) — single-user: one group id 0.
fn sys_getgroups(size: u64, list: u64) -> u64 {
    // size==0: return count only
    if size == 0 {
        return 1;
    }
    if size < 1 {
        return errno::neg(errno::EINVAL);
    }
    // gid_t is u32 on x86_64 Linux
    if !user_ptr_ok(list, 4) {
        return errno::neg(errno::EFAULT);
    }
    unsafe {
        core::ptr::write_volatile(list as *mut u32, 0);
    }
    1
}

/// Linux getcwd(buf, size) — returns length including NUL, or -ERANGE/-EFAULT.
fn sys_getcwd(buf: u64, size: u64) -> u64 {
    if size == 0 {
        return errno::neg(errno::ERANGE);
    }
    if size > 4096 {
        return errno::neg(errno::EINVAL);
    }
    if !user_ptr_ok(buf, size) {
        return errno::neg(errno::EFAULT);
    }
    let mut tmp = [0u8; 512];
    let n = crate::fs::path::getcwd_pretty(&mut tmp);
    // getcwd_pretty returns length without requiring trailing NUL in count;
    // ensure NUL and include it in returned length (Linux includes NUL).
    let mut len = n;
    if len >= tmp.len() {
        len = tmp.len() - 1;
    }
    tmp[len] = 0;
    let need = len + 1;
    if need as u64 > size {
        return errno::neg(errno::ERANGE);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), buf as *mut u8, need);
    }
    need as u64
}

/// Off-stack buffer for getdents64 (avoid nest-stack 4 KiB + dir_next frames).
static mut GETDENTS_TMP: [u8; 4096] = [0; 4096];

/// Linux getdents64(fd, dirp, count) — bytes written, 0 at EOF, or -errno.
fn sys_getdents64(fd: u64, dirp: u64, count: u64) -> u64 {
    if count == 0 {
        return errno::neg(errno::EINVAL);
    }
    let count = count.min(4096) as usize;
    if !user_ptr_ok(dirp, count as u64) {
        return errno::neg(errno::EFAULT);
    }
    let tmp = unsafe { &mut *core::ptr::addr_of_mut!(GETDENTS_TMP) };
    let n = match fd::sys_getdents64(fd, &mut tmp[..count]) {
        Ok(n) => n,
        Err(e) => return map_fd_err(e),
    };
    unsafe {
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), dirp as *mut u8, n);
    }
    n as u64
}

/// Linux chdir(path) — 0 or -errno.
fn sys_chdir(path_ptr: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    match crate::fs::path::chdir(path) {
        Ok(()) => 0,
        Err("not a directory") => errno::neg(errno::ENOTDIR),
        Err(_) => errno::neg(errno::ENOENT),
    }
}

fn map_fs_write_err(e: &str) -> u64 {
    match e {
        "exists" => errno::neg(errno::EEXIST),
        "not a directory" => errno::neg(errno::ENOTDIR),
        "is a directory (use rmdir)" | "is a directory" => errno::neg(errno::EISDIR),
        "directory not empty" => errno::neg(errno::ENOTEMPTY),
        "bad name" => errno::neg(errno::EINVAL),
        "not mounted" | "not found" | "no such" => errno::neg(errno::ENOENT),
        "too many symlinks" => errno::neg(errno::ELOOP),
        "not a symlink" => errno::neg(errno::EINVAL),
        _ => {
            // lookup / resolve failures
            if e.contains("not found") || e.contains("ENOENT") {
                errno::neg(errno::ENOENT)
            } else {
                errno::neg(errno::EINVAL)
            }
        }
    }
}

fn user_path_str<'a>(path_ptr: u64, buf: &'a mut [u8]) -> Result<&'a str, u64> {
    let n = copy_user_path(path_ptr, buf)?;
    core::str::from_utf8(&buf[..n]).map_err(|_| errno::neg(errno::ENOENT))
}

/// Linux mkdir(path, mode) — mode largely ignored (default 0755 in FS layer).
fn sys_mkdir(path_ptr: u64, _mode: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // cli: ext2 write uses static block scratches (not re-entrant with IRQ).
    let r = unsafe {
        asm!("cli", options(nomem, nostack));
        let r = crate::fs::vops::vfs_mkdir(path);
        asm!("sti", options(nomem, nostack));
        r
    };
    match r {
        Ok(()) => 0,
        Err(e) => map_fs_write_err(crate::fs::vops::vfs_err_str(e)),
    }
}

/// Linux mkdirat(dirfd, path, mode) — AT_FDCWD / absolute only for now.
fn sys_mkdirat(dirfd: u64, path_ptr: u64, mode: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    let _ = mode;
    sys_mkdir(path_ptr, mode)
}

/// Linux rmdir(path).
fn sys_rmdir(path_ptr: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let r = unsafe {
        asm!("cli", options(nomem, nostack));
        let r = crate::fs::vops::vfs_rmdir(path);
        asm!("sti", options(nomem, nostack));
        r
    };
    match r {
        Ok(()) => 0,
        Err(e) => map_fs_write_err(crate::fs::vops::vfs_err_str(e)),
    }
}

/// Linux unlink(path).
fn sys_unlink(path_ptr: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let r = unsafe {
        asm!("cli", options(nomem, nostack));
        let r = crate::fs::vops::vfs_unlink(path);
        asm!("sti", options(nomem, nostack));
        r
    };
    match r {
        Ok(()) => 0,
        Err(e) => map_fs_write_err(crate::fs::vops::vfs_err_str(e)),
    }
}

fn sys_rename(old_ptr: u64, new_ptr: u64) -> u64 {
    let mut a = [0u8; 256];
    let mut b = [0u8; 256];
    let old = match user_path_str(old_ptr, &mut a) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // copy old to stack string we own
    let mut old_owned = [0u8; 256];
    let ol = old.len().min(255);
    old_owned[..ol].copy_from_slice(old.as_bytes());
    let old = core::str::from_utf8(&old_owned[..ol]).unwrap_or("");
    let new = match user_path_str(new_ptr, &mut b) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let r = unsafe {
        asm!("cli", options(nomem, nostack));
        let r = crate::fs::vops::vfs_rename(old, new);
        asm!("sti", options(nomem, nostack));
        r
    };
    match r {
        Ok(()) => 0,
        Err(e) => map_fs_write_err(crate::fs::vops::vfs_err_str(e)),
    }
}

fn sys_renameat(olddirfd: u64, old_ptr: u64, newdirfd: u64, new_ptr: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    if olddirfd as i32 != AT_FDCWD || newdirfd as i32 != AT_FDCWD {
        // only AT_FDCWD for now unless absolute paths
        let mut a = [0u8; 256];
        let mut b = [0u8; 256];
        let old = match user_path_str(old_ptr, &mut a) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let new = match user_path_str(new_ptr, &mut b) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if !old.starts_with('/') || !new.starts_with('/') {
            return errno::neg(errno::ENOSYS);
        }
    }
    sys_rename(old_ptr, new_ptr)
}

fn sys_symlink(target_ptr: u64, link_ptr: u64) -> u64 {
    let mut a = [0u8; 256];
    let mut b = [0u8; 256];
    let target = match user_path_str(target_ptr, &mut a) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut t_owned = [0u8; 256];
    let tl = target.len().min(255);
    t_owned[..tl].copy_from_slice(target.as_bytes());
    let target = core::str::from_utf8(&t_owned[..tl]).unwrap_or("");
    let link = match user_path_str(link_ptr, &mut b) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let r = unsafe {
        asm!("cli", options(nomem, nostack));
        let r = crate::fs::vops::vfs_symlink(target, link);
        asm!("sti", options(nomem, nostack));
        r
    };
    match r {
        Ok(()) => 0,
        Err(e) => map_fs_write_err(crate::fs::vops::vfs_err_str(e)),
    }
}

fn sys_symlinkat(target_ptr: u64, newdirfd: u64, link_ptr: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    let mut b = [0u8; 256];
    let link = match user_path_str(link_ptr, &mut b) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !link.starts_with('/') && newdirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    sys_symlink(target_ptr, link_ptr)
}

fn sys_readlink(path_ptr: u64, buf_ptr: u64, bufsiz: u64) -> u64 {
    if bufsiz == 0 {
        return errno::neg(errno::EINVAL);
    }
    if bufsiz > 4096 {
        return errno::neg(errno::EINVAL);
    }
    if !user_ptr_ok(buf_ptr, bufsiz) {
        return errno::neg(errno::EFAULT);
    }
    let mut pbuf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut pbuf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let ino = match resolve_user_ino(path, false) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let mut tbuf = [0u8; 256];
    let n = match crate::fs::ext2::read_symlink(ino, &mut tbuf) {
        Ok(n) => n,
        Err("not a symlink") => return errno::neg(errno::EINVAL),
        Err(_) => return errno::neg(errno::ENOENT),
    };
    let copy = n.min(bufsiz as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(tbuf.as_ptr(), buf_ptr as *mut u8, copy);
    }
    copy as u64
}

fn sys_readlinkat(dirfd: u64, path_ptr: u64, buf_ptr: u64, bufsiz: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    let mut pbuf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut pbuf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    sys_readlink(path_ptr, buf_ptr, bufsiz)
}

/// Linux `struct statx` (uapi) — first 256 bytes used by musl/BusyBox.
const STATX_SIZE: usize = 256;
const STATX_BASIC_STATS: u32 = 0x0000_07ff;
const AT_SYMLINK_NOFOLLOW: u64 = 0x100;

fn sys_statx(dirfd: u64, path_ptr: u64, flags: u64, _mask: u64, buf_ptr: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    if !user_ptr_ok(buf_ptr, STATX_SIZE as u64) {
        return errno::neg(errno::EFAULT);
    }
    let mut pbuf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut pbuf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    let follow = flags & AT_SYMLINK_NOFOLLOW == 0;
    let st = match crate::fs::vcore::vfs_stat(path, follow) {
        Ok(s) => s,
        Err(e) => return map_vfs_stat_err(e),
    };
    let blksize = if st.blksize == 0 { 1024 } else { st.blksize };
    unsafe {
        core::ptr::write_bytes(buf_ptr as *mut u8, 0, STATX_SIZE);
    }
    let p = buf_ptr as *mut u8;
    write_u32_le(p, 0x00, STATX_BASIC_STATS);
    write_u32_le(p, 0x04, blksize);
    write_u32_le(p, 0x10, st.nlink as u32);
    write_u32_le(p, 0x14, st.uid as u32);
    write_u32_le(p, 0x18, st.gid as u32);
    write_u16_le(p, 0x1c, st.mode);
    write_u64_le(p, 0x20, st.ino);
    write_u64_le(p, 0x28, st.size);
    write_u64_le(p, 0x30, st.blocks_512);
    write_u64_le(p, 0x40, st.atime as u64);
    write_u64_le(p, 0x60, st.ctime as u64);
    write_u64_le(p, 0x70, st.mtime as u64);
    write_u32_le(p, 0x88, 0xdead);
    write_u32_le(p, 0x8c, st.rdev);
    0
}

fn write_u16_le(base: *mut u8, off: usize, v: u16) {
    unsafe {
        core::ptr::copy_nonoverlapping(v.to_le_bytes().as_ptr(), base.add(off), 2);
    }
}

fn sys_link(old_ptr: u64, new_ptr: u64) -> u64 {
    let mut a = [0u8; 256];
    let mut b = [0u8; 256];
    let old = match user_path_str(old_ptr, &mut a) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut old_owned = [0u8; 256];
    let ol = old.len().min(255);
    old_owned[..ol].copy_from_slice(old.as_bytes());
    let old = core::str::from_utf8(&old_owned[..ol]).unwrap_or("");
    let new = match user_path_str(new_ptr, &mut b) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let r = unsafe {
        asm!("cli", options(nomem, nostack));
        let r = crate::fs::vops::vfs_link(old, new);
        asm!("sti", options(nomem, nostack));
        r
    };
    match r {
        Ok(()) => 0,
        Err(e) => map_fs_write_err(crate::fs::vops::vfs_err_str(e)),
    }
}

/// Wait until `scan` reports >0 ready fds, timeout (ms) elapses, or timeout==0.
/// `timeout_ms < 0` waits indefinitely (capped spin).
fn wait_ready<F: FnMut() -> u64>(timeout_ms: i64, mut scan: F) -> u64 {
    let start = crate::interrupts::ticks();
    let ticks_need: Option<u64> = if timeout_ms < 0 {
        None
    } else if timeout_ms == 0 {
        Some(0)
    } else {
        Some(((timeout_ms as u64) + 9) / 10)
    };
    // Bound so a stuck wait cannot hang QEMU forever (~60s at 100 Hz).
    for _ in 0..6000 {
        crate::tty::deliver_pending_sigint();
        if let Some(sig) = crate::tty::take_force_fatal() {
            fatal_signal_exit(sig);
        }
        let n = scan();
        if n > 0 {
            return n;
        }
        if let Some(need) = ticks_need {
            if need == 0 || crate::interrupts::ticks().wrapping_sub(start) >= need {
                return 0;
            }
        }
        try_run_ready_child();
        unsafe {
            asm!("sti; hlt", options(nostack));
        }
    }
    0
}

/// Linux poll(fds, nfds, timeout_ms).
fn sys_poll(fds_ptr: u64, nfds: u64, timeout: u64) -> u64 {
    const MAX_POLL: u64 = 32;
    if nfds > MAX_POLL {
        return errno::neg(errno::EINVAL);
    }
    if nfds == 0 {
        let t = timeout as i64;
        if t == 0 {
            return 0;
        }
        return wait_ready(t, || 0);
    }
    let bytes = nfds.saturating_mul(8);
    if !user_ptr_ok(fds_ptr, bytes) {
        return errno::neg(errno::EFAULT);
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PollFd {
        fd: i32,
        events: u16,
        revents: u16,
    }
    let mut local = [PollFd {
        fd: -1,
        events: 0,
        revents: 0,
    }; 32];
    let n = nfds as usize;
    unsafe {
        core::ptr::copy_nonoverlapping(fds_ptr as *const PollFd, local.as_mut_ptr(), n);
    }
    let timeout_ms = timeout as i32 as i64;
    let ready = wait_ready(timeout_ms, || {
        let mut c = 0u64;
        for p in local[..n].iter_mut() {
            p.revents = crate::fd::poll::revents(p.fd, p.events);
            if p.revents != 0 {
                c += 1;
            }
        }
        c
    });
    unsafe {
        core::ptr::copy_nonoverlapping(local.as_ptr(), fds_ptr as *mut PollFd, n);
    }
    ready
}

/// Linux ppoll — timespec timeout; sigmask ignored.
fn sys_ppoll(fds: u64, nfds: u64, tsp: u64, _sigmask: u64) -> u64 {
    let timeout_ms = if tsp == 0 {
        -1i64
    } else {
        if !user_ptr_ok(tsp, 16) {
            return errno::neg(errno::EFAULT);
        }
        let sec = unsafe { core::ptr::read_volatile(tsp as *const i64) };
        let nsec = unsafe { core::ptr::read_volatile((tsp + 8) as *const i64) };
        if sec < 0 || nsec < 0 || nsec >= 1_000_000_000 {
            return errno::neg(errno::EINVAL);
        }
        if sec == 0 && nsec == 0 {
            0
        } else {
            sec.saturating_mul(1000)
                .saturating_add((nsec + 999_999) / 1_000_000)
        }
    };
    sys_poll(fds, nfds, timeout_ms as u64)
}

/// Linux select(nfds, readfds, writefds, exceptfds, timeout).
fn sys_select(nfds: u64, rfds: u64, wfds: u64, efds: u64, tv: u64) -> u64 {
    if nfds > 1024 {
        return errno::neg(errno::EINVAL);
    }
    let ncheck = (nfds as usize).min(crate::fd::FD_MAX);
    let set_bytes = 128usize; // FD_SETSIZE 1024 bits
    let mut rin = [0u8; 128];
    let mut win = [0u8; 128];
    let mut ein = [0u8; 128];
    if rfds != 0 {
        if !user_ptr_ok(rfds, set_bytes as u64) {
            return errno::neg(errno::EFAULT);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(rfds as *const u8, rin.as_mut_ptr(), set_bytes);
        }
    }
    if wfds != 0 {
        if !user_ptr_ok(wfds, set_bytes as u64) {
            return errno::neg(errno::EFAULT);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(wfds as *const u8, win.as_mut_ptr(), set_bytes);
        }
    }
    if efds != 0 {
        if !user_ptr_ok(efds, set_bytes as u64) {
            return errno::neg(errno::EFAULT);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(efds as *const u8, ein.as_mut_ptr(), set_bytes);
        }
    }
    fn bit(set: &[u8], fd: usize) -> bool {
        let byte = fd / 8;
        let mask = 1u8 << (fd % 8);
        byte < set.len() && set[byte] & mask != 0
    }
    fn set_bit(set: &mut [u8], fd: usize) {
        let byte = fd / 8;
        let mask = 1u8 << (fd % 8);
        if byte < set.len() {
            set[byte] |= mask;
        }
    }
    // EBADF if a requested fd is closed.
    for fd in 0..ncheck {
        let want_r = rfds != 0 && bit(&rin, fd);
        let want_w = wfds != 0 && bit(&win, fd);
        let want_e = efds != 0 && bit(&ein, fd);
        if !want_r && !want_w && !want_e {
            continue;
        }
        let rev = crate::fd::poll::revents(fd as i32, crate::fd::poll::POLLIN | crate::fd::poll::POLLOUT);
        if rev & crate::fd::poll::POLLNVAL != 0 {
            return errno::neg(errno::EBADF);
        }
    }
    let timeout_ms = if tv == 0 {
        -1i64
    } else {
        if !user_ptr_ok(tv, 16) {
            return errno::neg(errno::EFAULT);
        }
        let sec = unsafe { core::ptr::read_volatile(tv as *const i64) };
        let usec = unsafe { core::ptr::read_volatile((tv + 8) as *const i64) };
        if sec < 0 || usec < 0 {
            return errno::neg(errno::EINVAL);
        }
        sec.saturating_mul(1000).saturating_add((usec + 999) / 1000)
    };
    let ready = wait_ready(timeout_ms, || {
        let mut c = 0u64;
        for fd in 0..ncheck {
            let want_r = rfds != 0 && bit(&rin, fd);
            let want_w = wfds != 0 && bit(&win, fd);
            let want_e = efds != 0 && bit(&ein, fd);
            if !want_r && !want_w && !want_e {
                continue;
            }
            let mut ev = 0u16;
            if want_r {
                ev |= crate::fd::poll::POLLIN;
            }
            if want_w {
                ev |= crate::fd::poll::POLLOUT;
            }
            let rev = crate::fd::poll::revents(fd as i32, ev);
            if (want_r && rev & (crate::fd::poll::POLLIN | crate::fd::poll::POLLHUP | crate::fd::poll::POLLERR) != 0)
                || (want_w && rev & (crate::fd::poll::POLLOUT | crate::fd::poll::POLLERR) != 0)
                || (want_e && rev & crate::fd::poll::POLLERR != 0)
            {
                c += 1;
            }
        }
        c
    });
    let mut rout = [0u8; 128];
    let mut wout = [0u8; 128];
    let mut eout = [0u8; 128];
    if ready > 0 {
        for fd in 0..ncheck {
            let want_r = rfds != 0 && bit(&rin, fd);
            let want_w = wfds != 0 && bit(&win, fd);
            let want_e = efds != 0 && bit(&ein, fd);
            let mut ev = 0u16;
            if want_r {
                ev |= crate::fd::poll::POLLIN;
            }
            if want_w {
                ev |= crate::fd::poll::POLLOUT;
            }
            let rev = crate::fd::poll::revents(fd as i32, ev);
            if want_r && rev & (crate::fd::poll::POLLIN | crate::fd::poll::POLLHUP | crate::fd::poll::POLLERR) != 0
            {
                set_bit(&mut rout, fd);
            }
            if want_w && rev & (crate::fd::poll::POLLOUT | crate::fd::poll::POLLERR) != 0 {
                set_bit(&mut wout, fd);
            }
            if want_e && rev & crate::fd::poll::POLLERR != 0 {
                set_bit(&mut eout, fd);
            }
        }
    }
    if rfds != 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(rout.as_ptr(), rfds as *mut u8, set_bytes);
        }
    }
    if wfds != 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(wout.as_ptr(), wfds as *mut u8, set_bytes);
        }
    }
    if efds != 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(eout.as_ptr(), efds as *mut u8, set_bytes);
        }
    }
    ready
}

fn sys_epoll_create(size: u64) -> u64 {
    if size == 0 {
        return errno::neg(errno::EINVAL);
    }
    sys_epoll_create1(0)
}

fn sys_epoll_create1(flags: u64) -> u64 {
    match crate::fd::epoll::create_fd(flags as u32) {
        Ok(fd) => fd as u64,
        Err(e) => errno::neg(e),
    }
}

fn sys_epoll_ctl(epfd: u64, op: u64, fd: u64, event_ptr: u64) -> u64 {
    let mut events = 0u32;
    let mut data = 0u64;
    if op != crate::fd::epoll::EPOLL_CTL_DEL as u64 {
        // packed 12-byte epoll_event on x86_64
        if !user_ptr_ok(event_ptr, 12) {
            return errno::neg(errno::EFAULT);
        }
        unsafe {
            events = core::ptr::read_unaligned(event_ptr as *const u32);
            data = core::ptr::read_unaligned((event_ptr + 4) as *const u64);
        }
    }
    match crate::fd::epoll::ctl(epfd as usize, op as i32, fd as i32, events, data) {
        Ok(()) => 0,
        Err(e) => errno::neg(e),
    }
}

fn sys_epoll_wait(epfd: u64, events_ptr: u64, maxevents: u64, timeout: u64) -> u64 {
    if maxevents == 0 || maxevents > 32 {
        return errno::neg(errno::EINVAL);
    }
    if !user_ptr_ok(events_ptr, maxevents.saturating_mul(12)) {
        return errno::neg(errno::EFAULT);
    }
    let max = maxevents as usize;
    let timeout_ms = timeout as i32 as i64;
    let mut buf = [(0u32, 0u64); 32];
    wait_ready(timeout_ms, || {
        match crate::fd::epoll::collect(epfd as usize, &mut buf[..max]) {
            Ok(c) => c as u64,
            Err(_) => 0,
        }
    });
    // Re-collect to fill `buf` (level-triggered: still ready).
    let n = match crate::fd::epoll::collect(epfd as usize, &mut buf[..max]) {
        Ok(c) => c,
        Err(e) => return errno::neg(e),
    };
    for i in 0..n {
        let p = events_ptr + (i as u64) * 12;
        unsafe {
            core::ptr::write_unaligned(p as *mut u32, buf[i].0);
            core::ptr::write_unaligned((p + 4) as *mut u64, buf[i].1);
        }
    }
    n as u64
}

fn sys_pipe(pipefd_ptr: u64) -> u64 {
    if !user_ptr_ok(pipefd_ptr, 8) {
        return errno::neg(errno::EFAULT);
    }
    match crate::fd::sys_pipe() {
        Ok((r, w)) => {
            unsafe {
                core::ptr::write_volatile(pipefd_ptr as *mut i32, r as i32);
                core::ptr::write_volatile((pipefd_ptr + 4) as *mut i32, w as i32);
            }
            0
        }
        Err(e) => map_fd_err(e),
    }
}

fn sys_dup(fd: u64) -> u64 {
    match crate::fd::sys_dup(fd) {
        Ok(n) => n as u64,
        Err(e) => map_fd_err(e),
    }
}

fn sys_dup2(old: u64, new: u64) -> u64 {
    match crate::fd::sys_dup2(old, new) {
        Ok(n) => n as u64,
        Err(e) => map_fd_err(e),
    }
}

/// Linux unlinkat(dirfd, path, flags) — AT_FDCWD / absolute; AT_REMOVEDIR → rmdir.
fn sys_unlinkat(dirfd: u64, path_ptr: u64, flags: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    const AT_REMOVEDIR: u64 = 0x200;
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    if flags & AT_REMOVEDIR != 0 {
        sys_rmdir(path_ptr)
    } else {
        sys_unlink(path_ptr)
    }
}

/// Linux access(path, mode) — existence / pretend writable for single-user.
fn sys_access(path_ptr: u64, _mode: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2::resolve_path(cwd, path) {
        Ok(_) => 0,
        Err(_) => errno::neg(errno::ENOENT),
    }
}

fn sys_faccessat(dirfd: u64, path_ptr: u64, mode: u64, _flags: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    sys_access(path_ptr, mode)
}

/// Linux chmod — accept and ignore mode bits for now (single-user FS).
fn sys_chmod(path_ptr: u64, _mode: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2::resolve_path(cwd, path) {
        Ok(_) => 0,
        Err(_) => errno::neg(errno::ENOENT),
    }
}

fn sys_fchmodat(dirfd: u64, path_ptr: u64, mode: u64, _flags: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    sys_chmod(path_ptr, mode)
}

/// Linux umask — store ignored; return previous (always 0022).
fn sys_umask(_mask: u64) -> u64 {
    0o022
}

/// Linux nanosleep(req, rem) — coarse sleep using PIT ticks (~10 ms).
fn sys_nanosleep(req: u64, rem: u64) -> u64 {
    if !user_ptr_ok(req, 16) {
        return errno::neg(errno::EFAULT);
    }
    let sec = unsafe { core::ptr::read_volatile(req as *const i64) };
    let nsec = unsafe { core::ptr::read_volatile((req + 8) as *const i64) };
    if sec < 0 || nsec < 0 || nsec >= 1_000_000_000 {
        return errno::neg(errno::EINVAL);
    }
    // Convert to PIT ticks at 100 Hz.
    let total_ns = (sec as u64).saturating_mul(1_000_000_000).saturating_add(nsec as u64);
    let ticks_needed = (total_ns + 9_999_999) / 10_000_000; // ceil to 10ms
    let start = crate::interrupts::ticks();
    while crate::interrupts::ticks().wrapping_sub(start) < ticks_needed as u64 {
        // Ctrl-C / deferred fatal: exit nest-safely (was ignored → sleep forever).
        crate::tty::deliver_pending_sigint();
        if let Some(sig) = crate::tty::take_force_fatal() {
            fatal_signal_exit(sig);
        }
        unsafe {
            // Allow timer IRQ while halted; re-check force_fatal after wake.
            asm!("sti; hlt", options(nostack));
        }
    }
    if rem != 0 && user_ptr_ok(rem, 16) {
        unsafe {
            core::ptr::write_volatile(rem as *mut i64, 0);
            core::ptr::write_volatile((rem + 8) as *mut i64, 0);
        }
    }
    0
}

/// Linux utimensat — update times; create empty file if missing (BusyBox touch).
/// Linux itself does not create on ENOENT; BusyBox then open(O_CREAT). We accept
/// either path: create-on-utimensat OR create-on-open both go through `touch`.
fn sys_utimensat(dirfd: u64, path_ptr: u64, _times: u64, _flags: u64) -> u64 {
    const AT_FDCWD: i32 = -100;
    // path_ptr == 0 means "dirfd itself" — not supported without fstat path.
    if path_ptr == 0 {
        return 0;
    }
    let mut path_buf = [0u8; 256];
    let path = match user_path_str(path_ptr, &mut path_buf) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !path.starts_with('/') && dirfd as i32 != AT_FDCWD {
        return errno::neg(errno::ENOSYS);
    }
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2_write::touch(cwd, path) {
        Ok(_) => 0,
        Err(e) => map_fs_write_err(e),
    }
}

fn sys_futimesat(dirfd: u64, path_ptr: u64, _times: u64) -> u64 {
    sys_utimensat(dirfd, path_ptr, 0, 0)
}

fn sys_utimes(path_ptr: u64, _times: u64) -> u64 {
    sys_utimensat((-100i32) as u64, path_ptr, 0, 0)
}

// ---------------------------------------------------------------------------
// Phase 8 — init_module / delete_module / finit_module (Linux numbers)
// ---------------------------------------------------------------------------

fn module_err_to_errno(e: crate::module::ModuleError) -> u64 {
    use crate::module::ModuleError;
    use crate::module::mnx::MnxError;
    match e {
        ModuleError::Exists => errno::neg(errno::EEXIST),
        ModuleError::NotFound => errno::neg(errno::ENOENT),
        ModuleError::Busy => errno::neg(errno::EBUSY),
        ModuleError::NoSlot | ModuleError::Format(MnxError::Oom) => errno::neg(errno::ENOMEM),
        ModuleError::BadPath | ModuleError::Format(MnxError::BadName) => errno::neg(errno::EINVAL),
        ModuleError::Io => errno::neg(errno::EFAULT),
        ModuleError::InitFail | ModuleError::Format(MnxError::InitFail) => {
            errno::neg(errno::EPERM)
        }
        ModuleError::Format(_) => errno::neg(errno::ENOEXEC),
    }
}

/// `long init_module(void *umod, unsigned long len, const char *uargs);`
///
/// `uargs` (module parameters) is accepted and ignored for now.
fn sys_init_module(umod: u64, len: u64, _uargs: u64) -> u64 {
    if len == 0 || len > crate::module::mnx::MNX_MAX_FILE as u64 {
        return errno::neg(errno::EINVAL);
    }
    if !user_ptr_ok(umod, len) {
        return errno::neg(errno::EFAULT);
    }
    // Copy into kernel buffer so reloc/init never touch user pages mid-flight.
    let kbuf = match crate::memory::kmalloc(len as usize) {
        Some(p) => p,
        None => return errno::neg(errno::ENOMEM),
    };
    unsafe {
        core::ptr::copy_nonoverlapping(umod as *const u8, kbuf, len as usize);
    }
    let slice = unsafe { core::slice::from_raw_parts(kbuf, len as usize) };
    let rc = match crate::module::init_from_image(slice, "") {
        Ok(()) => 0u64,
        Err(e) => module_err_to_errno(e),
    };
    crate::memory::kfree(kbuf);
    rc
}

/// `long delete_module(const char *name_user, unsigned int flags);`
fn sys_delete_module(name_ptr: u64, _flags: u64) -> u64 {
    let mut name_buf = [0u8; 64];
    let n = match copy_user_path(name_ptr, &mut name_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let name = match core::str::from_utf8(&name_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::EINVAL),
    };
    match crate::module::rmmod(name) {
        Ok(()) => 0,
        Err(e) => module_err_to_errno(e),
    }
}

/// `long finit_module(int fd, const char *uargs, int flags);`
///
/// Reads the open file into a kernel buffer and loads it (Linux-compatible
/// entry point used by modern userspace `insmod`).
fn sys_finit_module(fd: u64, _uargs: u64, _flags: u64) -> u64 {
    let max = crate::module::mnx::MNX_MAX_FILE;
    let kbuf = match crate::memory::kmalloc(max) {
        Some(p) => p,
        None => return errno::neg(errno::ENOMEM),
    };
    let mut total = 0usize;
    loop {
        if total >= max {
            break;
        }
        let slice = unsafe {
            core::slice::from_raw_parts_mut(kbuf.add(total), max - total)
        };
        match fd::sys_read_into(fd, slice) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(fd::FdError::BadFd) => {
                crate::memory::kfree(kbuf);
                return errno::neg(errno::EBADF);
            }
            Err(_) => {
                crate::memory::kfree(kbuf);
                return errno::neg(errno::EFAULT);
            }
        }
    }
    if total == 0 {
        crate::memory::kfree(kbuf);
        return errno::neg(errno::EINVAL);
    }
    let slice = unsafe { core::slice::from_raw_parts(kbuf, total) };
    let rc = match crate::module::init_from_image(slice, "") {
        Ok(()) => 0u64,
        Err(e) => module_err_to_errno(e),
    };
    crate::memory::kfree(kbuf);
    rc
}

// ENOTDIR used above
// add to errno module - I used ENOTDIR without defining it
fn copy_user_path(path_ptr: u64, out: &mut [u8]) -> Result<usize, u64> {
    copy_user_cpath(path_ptr, out, false)
}

/// Copy a NUL-terminated user path. Empty string is `ENOENT` unless `allow_empty`
/// (needed for `execveat(..., "", ..., AT_EMPTY_PATH)` / `fexecve`).
fn copy_user_cpath(path_ptr: u64, out: &mut [u8], allow_empty: bool) -> Result<usize, u64> {
    if path_ptr == 0 {
        return Err(errno::neg(errno::EFAULT));
    }
    // Copy until NUL or out full (leave room for safety)
    let max = out.len().saturating_sub(1);
    let mut n = 0usize;
    while n < max {
        if !user_ptr_ok(path_ptr + n as u64, 1) {
            return Err(errno::neg(errno::EFAULT));
        }
        let b = unsafe { core::ptr::read_volatile((path_ptr as usize + n) as *const u8) };
        if b == 0 {
            break;
        }
        out[n] = b;
        n += 1;
    }
    if n == 0 && !allow_empty {
        return Err(errno::neg(errno::ENOENT));
    }
    // If we filled max without NUL, path too long
    if n == max {
        let next_ok = user_ptr_ok(path_ptr + n as u64, 1);
        if next_ok {
            let b = unsafe { core::ptr::read_volatile((path_ptr as usize + n) as *const u8) };
            if b != 0 {
                return Err(errno::neg(errno::ENAMETOOLONG));
            }
        }
    }
    Ok(n)
}

/// Ensure a user-accessible page at `virt` (see `elf::map_user_page`).
fn map_user_page(virt: u64) -> Result<(), &'static str> {
    crate::elf::map_user_page(virt).map_err(|_| "map_user_page")
}

fn setup_demo_image() -> Result<(), &'static str> {
    map_user_page(DEMO_CODE)?;
    map_user_page(DEMO_STACK_PAGE)?;

    let prog = user_demo_bytes();
    unsafe {
        core::ptr::write_bytes(DEMO_CODE as *mut u8, 0, FRAME_SIZE);
        core::ptr::copy_nonoverlapping(prog.as_ptr(), DEMO_CODE as *mut u8, prog.len());
        core::ptr::write_bytes(DEMO_STACK_PAGE as *mut u8, 0, FRAME_SIZE);
    }
    Ok(())
}

/// Hand-assembled: write(1, msg, n); exit(0);
fn user_demo_bytes() -> [u8; 256] {
    let mut out = [0u8; 256];
    let msg = b"Hello from ring 3 via syscall!\n";
    // Place message at CODE+0x80
    out[0x80..0x80 + msg.len()].copy_from_slice(msg);
    let msg_addr = DEMO_CODE + 0x80;
    let msg_len = msg.len() as u32;

    let mut i = 0usize;
    // mov rax, 1  (Linux write)
    out[i] = 0x48;
    out[i + 1] = 0xC7;
    out[i + 2] = 0xC0;
    out[i + 3..i + 7].copy_from_slice(&1u32.to_le_bytes());
    i += 7;
    // mov rdi, 1
    out[i] = 0x48;
    out[i + 1] = 0xC7;
    out[i + 2] = 0xC7;
    out[i + 3..i + 7].copy_from_slice(&1u32.to_le_bytes());
    i += 7;
    // mov rsi, msg_addr
    out[i] = 0x48;
    out[i + 1] = 0xBE;
    out[i + 2..i + 10].copy_from_slice(&msg_addr.to_le_bytes());
    i += 10;
    // mov rdx, msg_len
    out[i] = 0x48;
    out[i + 1] = 0xC7;
    out[i + 2] = 0xC2;
    out[i + 3..i + 7].copy_from_slice(&msg_len.to_le_bytes());
    i += 7;
    // syscall
    out[i] = 0x0F;
    out[i + 1] = 0x05;
    i += 2;
    // mov rax, 60 (Linux exit)
    out[i] = 0x48;
    out[i + 1] = 0xC7;
    out[i + 2] = 0xC0;
    out[i + 3..i + 7].copy_from_slice(&60u32.to_le_bytes());
    i += 7;
    // xor rdi, rdi
    out[i] = 0x48;
    out[i + 1] = 0x31;
    out[i + 2] = 0xFF;
    i += 3;
    // syscall
    out[i] = 0x0F;
    out[i + 1] = 0x05;
    let _ = i;
    out
}

/// Enter a Ready task's user context (current process must already be that task).
fn run_user_frame(frame: crate::process::UserFrame) {
    crate::process::sched::clear_need_resched();
    enter_user_nested(frame.rip, frame.rsp, frame.rax);
}

/// Linux fork() — parent returns child pid; child is left **Ready**.
///
/// Phase 3b: does **not** run the child inside fork. The parent’s `wait4`
/// (or another schedule point) picks Ready children via [`sched::take_ready`].
fn sys_fork() -> u64 {
    let (rip, rsp, rflags) = unsafe {
        (
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rip)),
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rsp)),
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rflags)),
        )
    };
    match crate::process::fork_from_user(rip, rsp, rflags) {
        Ok(pid) => pid as u64,
        Err(_) => errno::neg(errno::EAGAIN),
    }
}

/// Linux clone(flags, stack, parent_tid, child_tid, tls) — Phase 4.
///
/// Parent returns child tid; child is **Ready** with `rax=0` (like fork).
fn sys_clone(flags: u64, stack: u64, parent_tid: u64, child_tid: u64, tls: u64) -> u64 {
    let (rip, rsp, rflags) = unsafe {
        (
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rip)),
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rsp)),
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rflags)),
        )
    };
    let frame = child_trap_from_syscall(rip, if stack != 0 { stack } else { rsp }, rflags, 0);
    match crate::process::clone_from_user(
        flags,
        stack,
        parent_tid,
        child_tid,
        tls,
        rip,
        rsp,
        rflags,
        frame,
    ) {
        Ok(tid) => tid as u64,
        Err(_) => errno::neg(errno::EAGAIN),
    }
}

/// Linux clone3(struct clone_args *uargs, size_t size) — glibc pthread_create.
///
/// `stack` is the **low** address; user RSP = stack + stack_size.
fn sys_clone3(uargs: u64, size: u64) -> u64 {
    const VER0: u64 = 64; // flags..tls
    if size < VER0 || size > 256 {
        return errno::neg(errno::EINVAL);
    }
    if !user_ptr_ok(uargs, size) {
        return errno::neg(errno::EFAULT);
    }
    let rd = |off: u64| unsafe { core::ptr::read_volatile((uargs + off) as *const u64) };
    let flags = rd(0);
    let child_tid = rd(16);
    let parent_tid = rd(24);
    // clone3 puts CSIGNAL in exit_signal, not flags[7:0].
    let exit_signal = rd(32) & 0xff;
    let stack = rd(40);
    let stack_size = rd(48);
    let tls = rd(56);
    let child_rsp = if stack == 0 {
        0
    } else if stack_size == 0 {
        stack
    } else {
        stack.saturating_add(stack_size)
    };
    sys_clone(flags | exit_signal, child_rsp, parent_tid, child_tid, tls)
}

const ARGV_MAX: usize = 6;
const AT_EMPTY_PATH: u64 = 0x1000;

/// Linux execve(path, argv, envp) — envp ignored; argv up to 6 user strings.
/// On success does not return to the old image (nested enter + exit chain).
fn sys_execve(path_ptr: u64, argv_ptr: u64, _envp: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    do_execve_path(path, argv_ptr)
}

/// Linux execveat(dirfd, pathname, argv, envp, flags).
///
/// Supports `AT_FDCWD` / absolute path (same as `execve`), relative path from a
/// directory fd, and `AT_EMPTY_PATH` (`fexecve` — execute the open file).
/// `AT_SYMLINK_NOFOLLOW` fails with `ELOOP` if the last component is a symlink.
fn sys_execveat(dirfd: u64, path_ptr: u64, argv_ptr: u64, _envp: u64, flags: u64) -> u64 {
    const ALLOWED: u64 = AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW;
    if flags & !ALLOWED != 0 {
        return errno::neg(errno::EINVAL);
    }
    let mut path_buf = [0u8; 256];
    let n = match copy_user_cpath(path_ptr, &mut path_buf, true) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };
    let follow_last = flags & AT_SYMLINK_NOFOLLOW == 0;

    if path.is_empty() {
        if flags & AT_EMPTY_PATH == 0 {
            return errno::neg(errno::ENOENT);
        }
        return do_execve_fd(dirfd, argv_ptr);
    }
    if path.starts_with('/') || dirfd as i32 == -100 {
        if !follow_last {
            match resolve_user_ino(path, false) {
                Ok(ino) => {
                    if crate::fs::ext2::inode_is_lnk(ino) {
                        return errno::neg(errno::ELOOP);
                    }
                }
                Err(e) => return e,
            }
        }
        return do_execve_path(path, argv_ptr);
    }
    // Relative to dirfd (must be an open directory).
    let dir_ino = match crate::fd::sys_fd_inode(dirfd) {
        Ok(i) => i,
        Err(crate::fd::FdError::BadFd) => return errno::neg(errno::EBADF),
        Err(_) => return errno::neg(errno::ENOTDIR),
    };
    if !crate::fs::ext2::inode_is_dir(dir_ino) {
        return errno::neg(errno::ENOTDIR);
    }
    let ino = match if follow_last {
        crate::fs::ext2::resolve_path(dir_ino, path)
    } else {
        crate::fs::ext2::resolve_lpath(dir_ino, path)
    } {
        Ok(i) => i,
        Err("too many symlinks") => return errno::neg(errno::ELOOP),
        Err("not a directory") => return errno::neg(errno::ENOTDIR),
        Err(_) => return errno::neg(errno::ENOENT),
    };
    if !follow_last && crate::fs::ext2::inode_is_lnk(ino) {
        return errno::neg(errno::ELOOP);
    }
    if crate::fs::ext2::inode_is_dir(ino) {
        return errno::neg(errno::EACCES);
    }
    let hint = path.rsplit('/').next().unwrap_or(path);
    do_execve_ino(ino, hint, argv_ptr)
}

fn do_execve_fd(fd: u64, argv_ptr: u64) -> u64 {
    let ino = match crate::fd::sys_fd_inode(fd) {
        Ok(i) => i,
        Err(crate::fd::FdError::BadFd) => return errno::neg(errno::EBADF),
        Err(_) => return errno::neg(errno::EACCES),
    };
    if crate::fs::ext2::inode_is_dir(ino) {
        return errno::neg(errno::EACCES);
    }
    do_execve_ino(ino, "fdexec", argv_ptr)
}

fn do_execve_path(path: &str, argv_ptr: u64) -> u64 {
    let (abuf, arg_lens, argc) = collect_argv(path, argv_ptr);
    let mut refs: [&str; ARGV_MAX] = [""; ARGV_MAX];
    for i in 0..argc {
        refs[i] = core::str::from_utf8(&abuf[i][..arg_lens[i]]).unwrap_or("?");
    }
    let argv_slice = &refs[..argc];
    let image = match load_exec_image(path, argv_slice) {
        Ok(img) => img,
        Err(e) => return map_exec_err(e),
    };
    commit_exec(image, refs[0])
}

fn do_execve_ino(ino: u32, argv0_hint: &str, argv_ptr: u64) -> u64 {
    let (abuf, arg_lens, argc) = collect_argv(argv0_hint, argv_ptr);
    let mut refs: [&str; ARGV_MAX] = [""; ARGV_MAX];
    for i in 0..argc {
        refs[i] = core::str::from_utf8(&abuf[i][..arg_lens[i]]).unwrap_or("?");
    }
    let argv_slice = &refs[..argc];
    let image = match load_elf_from_ino(ino, argv_slice) {
        Ok(img) => img,
        Err(e) => return map_exec_err(e),
    };
    commit_exec(image, refs[0])
}

fn collect_argv(argv0_default: &str, argv_ptr: u64) -> ([[u8; 64]; ARGV_MAX], [usize; ARGV_MAX], usize) {
    let mut abuf = [[0u8; 64]; ARGV_MAX];
    let mut arg_lens = [0usize; ARGV_MAX];
    let dlen = core::cmp::min(argv0_default.len(), 63);
    abuf[0][..dlen].copy_from_slice(&argv0_default.as_bytes()[..dlen]);
    arg_lens[0] = dlen;
    let mut argc = 1usize;
    if argv_ptr != 0 && user_ptr_ok(argv_ptr, 8) {
        let mut n = 0usize;
        for i in 0..ARGV_MAX as u64 {
            let p = unsafe { core::ptr::read_volatile((argv_ptr + i * 8) as *const u64) };
            if p == 0 {
                break;
            }
            match copy_user_path(p, &mut abuf[i as usize]) {
                Ok(len) => {
                    arg_lens[i as usize] = core::cmp::min(len, 63);
                    n = i as usize + 1;
                }
                Err(_) => break,
            }
        }
        if n > 0 {
            argc = n;
        }
    }
    (abuf, arg_lens, argc)
}

fn map_exec_err(e: &str) -> u64 {
    match e {
        "no filesystem" | "not found" | "ENOENT" => errno::neg(errno::ENOENT),
        "is a directory" => errno::neg(errno::EACCES),
        "OOM" | "elf: OOM page" | "oom" => errno::neg(errno::ENOMEM),
        "elf: entry page zero" | "elf: entry page unmapped" | "elf: map vanished" => {
            errno::neg(errno::ENOEXEC)
        }
        _ if e.starts_with("elf:") => errno::neg(errno::ENOEXEC),
        _ => errno::neg(errno::ENOENT),
    }
}

/// Install a newly loaded ELF and enter it. Does not return on the success path
/// that the parent later resumes via nest / trap.
fn commit_exec(image: crate::elf::LoadedImage, argv0: &str) -> u64 {
    crate::process::clear_mmaps();

    let _ = crate::process::with_current(|p| {
        p.set_name(argv0);
        p.user_rip = image.entry;
        p.user_rsp = image.stack_top;
        p.user_rax = 0;
        p.user_rflags = 0x202;
        // New image: musl will re-set TLS; do not inherit previous FS base.
        p.fs_base = 0;
        p.gs_base = 0;
        // Fresh heap from ELF image end (Linux start_brk).
        p.heap_base = image.brk_start;
        p.heap_size = 0;
        // Clean GPRs so enter_user_mode does not leak the old image's regs.
        p.trap = crate::process::TrapFrame::from_user_entry(
            image.entry,
            image.stack_top,
            0x202,
            0,
        );
        p.trap_valid = true;
        // dumpable / no_new_privs / pdeathsig kept across exec (Linux).
    });
    crate::x86::msr::set_fs_base(0);
    crate::x86::msr::set_gs_base(0);

    enter_user_nested(image.entry, image.stack_top, 0);
    extern "C" {
        fn get_enter_nest_depth() -> u64;
    }
    let depth = unsafe { get_enter_nest_depth() };
    if depth > 1 {
        unsafe {
            return_from_user();
        }
    }
    resume_current_from_trap();
}

fn load_exec_image(path: &str, argv: &[&str]) -> Result<crate::elf::LoadedImage, &'static str> {
    let argv0 = if argv.is_empty() { "?" } else { argv[0] };
    // Prefer filesystem; fall back to embedded known binaries.
    if crate::fs::is_ready() {
        match load_elf_from_fs(path, argv) {
            Ok(img) => return Ok(img),
            // Surface real ELF/IO errors (not just "not found") so BusyBox-sized
            // loads are not silently turned into ENOENT via the embedded path.
            Err(e) if e != "not found" && e != "ENOENT" => return Err(e),
            Err(_) => {}
        }
        // try absolute /bin/*
        if !path.starts_with('/') {
            let mut abs = [0u8; 128];
            let prefix = b"/bin/";
            if prefix.len() + path.len() < abs.len() {
                abs[..prefix.len()].copy_from_slice(prefix);
                abs[prefix.len()..prefix.len() + path.len()]
                    .copy_from_slice(path.as_bytes());
                if let Ok(s) = core::str::from_utf8(&abs[..prefix.len() + path.len()]) {
                    match load_elf_from_fs(s, argv) {
                        Ok(img) => return Ok(img),
                        Err(e) if e != "not found" && e != "ENOENT" => return Err(e),
                        Err(_) => {}
                    }
                }
            }
        }
    }
    // Embedded fallbacks
    let load = |bytes: &'static [u8]| crate::elf::load_bytes_argv(bytes, argv);
    if path == "hello"
        || path == "/bin/hello"
        || path == "bin/hello"
        || path.ends_with("/hello")
    {
        return load(crate::embedded_hello::HELLO_ELF);
    }
    if path == "echo" || path == "/bin/echo" || path.ends_with("/echo") {
        return load(crate::embedded_echo::ECHO_ELF);
    }
    if path == "cat" || path == "/bin/cat" || path.ends_with("/cat") {
        return load(crate::embedded_cat::CAT_ELF);
    }
    if path == "ls" || path == "/bin/ls" || path.ends_with("/ls") {
        return load(crate::embedded_ls::LS_ELF);
    }
    if path == "forktest" || path == "/bin/forktest" || path.ends_with("/forktest") {
        return load(crate::embedded_forktest::FORKTEST_ELF);
    }
    if path == "exectest" || path == "/bin/exectest" || path.ends_with("/exectest") {
        return load(crate::embedded_exectest::EXECTEST_ELF);
    }
    if path == "sh" || path == "/bin/sh" || path == "bin/sh" || path.ends_with("/sh") {
        return load(crate::embedded_sh::SH_ELF);
    }
    if path == "vi" || path == "/bin/vi" || path == "vim" || path.ends_with("/vi") || path.ends_with("/vim")
    {
        return load(crate::embedded_vi::VI_ELF);
    }
    if path == "uname" || path == "/bin/uname" || path.ends_with("/uname") {
        return load(crate::embedded_uname::UNAME_ELF);
    }
    if path == "archprctl"
        || path == "/bin/archprctl"
        || path.ends_with("/archprctl")
    {
        return load(crate::embedded_archprctl::ARCHPRCTL_ELF);
    }
    if path == "brktest"
        || path == "/bin/brktest"
        || path.ends_with("/brktest")
    {
        return load(crate::embedded_brktest::BRKTEST_ELF);
    }
    if path == "mmaptest"
        || path == "/bin/mmaptest"
        || path.ends_with("/mmaptest")
    {
        return load(crate::embedded_mmaptest::MMAPTEST_ELF);
    }
    if path == "polltest"
        || path == "/bin/polltest"
        || path.ends_with("/polltest")
    {
        return load(crate::embedded_polltest::POLLTEST_ELF);
    }
    if path == "p9test"
        || path == "/bin/p9test"
        || path.ends_with("/p9test")
    {
        return load(crate::embedded_p9test::P9TEST_ELF);
    }
    if path == "preempttest"
        || path == "/bin/preempttest"
        || path.ends_with("/preempttest")
    {
        return load(crate::embedded_preempttest::PREEMPTTEST_ELF);
    }
    if path == "clonetest"
        || path == "/bin/clonetest"
        || path.ends_with("/clonetest")
    {
        return load(crate::embedded_clonetest::CLONETEST_ELF);
    }
    if path == "futextest"
        || path == "/bin/futextest"
        || path.ends_with("/futextest")
    {
        return load(crate::embedded_futextest::FUTEXTEST_ELF);
    }
    if path == "signaltest"
        || path == "/bin/signaltest"
        || path.ends_with("/signaltest")
    {
        return load(crate::embedded_signaltest::SIGNALTEST_ELF);
    }
    let _ = argv0;
    Err("ENOENT")
}

fn load_elf_from_fs(path: &str, argv: &[&str]) -> Result<crate::elf::LoadedImage, &'static str> {
    if !crate::fs::is_ready() {
        return Err("no filesystem");
    }
    let cwd = crate::fs::path::cwd_inode();
    let ino = crate::fs::ext2::resolve_path(cwd, path).map_err(|_| "not found")?;
    load_elf_from_ino(ino, argv)
}

fn load_elf_from_ino(ino: u32, argv: &[&str]) -> Result<crate::elf::LoadedImage, &'static str> {
    crate::elf::load_from_ino(ino, argv)
}

/// Linux wait4(pid, status, options, rusage) — rusage ignored.
/// Reaps zombies. Also schedules any leftover Ready children (if fork did not
/// run them), then reaps.
fn sys_wait4(pid: u64, status_ptr: u64, options: u64) -> u64 {
    const WNOHANG: u64 = 1;
    let wait_for = pid as i32;
    let nohang = (options & WNOHANG) != 0;

    // Blocking wait: run Ready children until one zombies or none left to run.
    for _ in 0..32 {
        let mut status = 0i32;
        let got = crate::process::waitpid(wait_for, Some(&mut status), true);

        if got > 0 {
            if status_ptr != 0 {
                if !user_ptr_ok(status_ptr, 4) {
                    return errno::neg(errno::EFAULT);
                }
                unsafe {
                    core::ptr::write_volatile(status_ptr as *mut i32, status);
                }
            }
            return got as u64;
        }
        if got < 0 {
            return errno::neg(errno::ECHILD);
        }
        // got == 0: children exist, none zombie yet
        if nohang {
            return 0;
        }
        // Schedule a Ready child (or preferred pid) to make progress.
        if let Some(frame) = crate::process::sched::take_ready(wait_for) {
            run_user_frame(frame);
            // Child exited → current is parent again; loop to reap.
            continue;
        }
        // No Ready child: nothing we can run cooperatively.
        return 0;
    }
    0
}

fn enter_and_wait(entry: u64, stack_top: u64, brk_start: u64, label: &str) -> Result<(), &'static str> {
    enter_and_wait_opts(entry, stack_top, brk_start, label, false)
}

/// `quiet`: suppress pid/entry chatter (used for clean U8 boot handoff).
fn enter_and_wait_opts(
    entry: u64,
    stack_top: u64,
    brk_start: u64,
    label: &str,
    quiet: bool,
) -> Result<(), &'static str> {
    // U5/U6: run as a child of init (shell) so getpid/exit/wait/fork are real
    let child = match crate::process::begin_user_task(label) {
        Ok(p) => p,
        Err(_) => {
            console::println("user: process table full");
            return Err("process table full");
        }
    };

    // Base level (shell → first user task). Syscalls from that task use the
    // dedicated TSS/kernel stack (not a nest slot). Nested fork/exec children
    // get push_syscall_stack() so they do not clobber this frame.
    unsafe {
        SYSCALL_STACK_DEPTH = 0;
    }
    ensure_kstack_base();

    // Program break for this image (heap starts empty at brk_start).
    crate::process::clear_mmaps();
    crate::process::set_brk_start(brk_start);

    if !quiet {
        console::print(label);
        console::print(" pid=");
        console::write_u64(child as u64);
        console::print(" entry=");
        console::write_hex64(entry);
        console::print(" stack=");
        console::write_hex64(stack_top);
        console::println("");
    }

    crate::process::apply_tls();
    let _ = crate::process::with_current(|p| {
        p.entered_via_nest = true;
        p.user_rip = entry;
        p.user_rsp = stack_top;
        p.user_rax = 0;
        p.user_rflags = 0x202;
        p.trap = crate::process::TrapFrame::from_user_entry(entry, stack_top, 0x202, 0);
        p.trap_valid = true;
    });
    unsafe {
        enter_user_frame =
            crate::process::TrapFrame::from_user_entry(entry, stack_top, 0x202, 0);
        enter_user_mode(entry, stack_top, 0);
    }

    unsafe {
        asm!(
            "mov ax, {kd}",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "sti",
            kd = const gdt::KERNEL_DATA_SELECTOR,
            options(nostack)
        );
    }

    // exit_user already switched current back to parent; reap zombie
    if let Some((pid, code)) = crate::process::reap_any_child() {
        if !quiet {
            console::print("user: exited pid=");
            console::write_u64(pid as u64);
            console::print(" status=");
            console::write_u64(code as u64);
            console::println("");
        }
    } else if !quiet {
        console::println("user: returned to kernel (no zombie?)");
    }
    Ok(())
}

/// Run the built-in hand-assembled ring-3 demo until exit.
pub fn run_demo_user_program() -> Result<(), &'static str> {
    setup_demo_image()?;
    // Demo blob is tiny; heap starts just after the demo page.
    enter_and_wait(DEMO_CODE, DEMO_STACK_TOP, DEMO_STACK_PAGE, "user: demo")
}

/// Load an ELF64 image from bytes and run until exit.
pub fn exec_elf_bytes(file: &[u8], argv0: &str) -> Result<(), &'static str> {
    let image = crate::elf::load_bytes(file, argv0)?;
    enter_and_wait(
        image.entry,
        image.stack_top,
        image.brk_start,
        "exec: ELF64",
    )
}

/// Run the embedded static `hello` ELF (built by `make userland` / `make build`).
pub fn run_embedded_hello() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_hello::HELLO_ELF, "hello")
}

/// Run embedded `echo` (READ stdin + WRITE stdout) — U2 test.
pub fn run_embedded_echo() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_echo::ECHO_ELF, "echo")
}

/// Run embedded `cat` (OPEN/READ file + WRITE) — U3 test (expects hello.txt on FS).
pub fn run_embedded_cat() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_cat::CAT_ELF, "cat")
}

/// Run embedded `ls` (open . + getdents64) — U4 test.
pub fn run_embedded_ls() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_ls::LS_ELF, "ls")
}

/// Run embedded `forktest` (fork + wait4) — U6 test.
pub fn run_embedded_forktest() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_forktest::FORKTEST_ELF, "forktest")
}

pub fn run_embedded_preempttest() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_preempttest::PREEMPTTEST_ELF, "preempttest")
}

pub fn run_embedded_clonetest() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_clonetest::CLONETEST_ELF, "clonetest")
}

pub fn run_embedded_futextest() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_futextest::FUTEXTEST_ELF, "futextest")
}

pub fn run_embedded_signaltest() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_signaltest::SIGNALTEST_ELF, "signaltest")
}

/// Run embedded `exectest` (fork + execve + wait4) — U6 test.
pub fn run_embedded_exectest() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_exectest::EXECTEST_ELF, "exectest")
}

/// Run embedded `uname` (UTS name fields).
pub fn run_embedded_uname() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_uname::UNAME_ELF, "uname")
}

/// Run embedded `/bin/sh` (U7 user shell).
pub fn run_embedded_sh() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_sh::SH_ELF, "sh")
}

/// Run `/bin/sh` with preloaded stdin (automated U7 smoke).
pub fn run_embedded_sh_script(script: &[u8]) -> Result<(), &'static str> {
    crate::interrupts::keyboard::init::inject_str(script);
    exec_elf_bytes(crate::embedded_sh::SH_ELF, "sh")
}

/// U8: boot handoff — start userspace `/bin/sh` as the first interactive program.
///
/// Prefer ext2 `/bin/sh`, fall back to the embedded ELF. Runs as a child of
/// kernel init (pid 1 = `kinit`). When the shell `exit`s, control returns here
/// so the caller can drop into the kernel debug shell.
pub fn run_init_sh() -> Result<(), &'static str> {
    let image = load_sh_image()?;
    enter_and_wait_opts(image.entry, image.stack_top, image.brk_start, "sh", true)
}

/// Load `/bin/sh` from the rootfs, or the embedded image if the disk path fails.
fn load_sh_image() -> Result<crate::elf::LoadedImage, &'static str> {
    if crate::fs::is_ready() {
        if let Ok(img) = load_elf_image_from_fs("/bin/sh", "sh") {
            return Ok(img);
        }
        if let Ok(img) = load_elf_image_from_fs("bin/sh", "sh") {
            return Ok(img);
        }
    }
    crate::elf::load_bytes(crate::embedded_sh::SH_ELF, "sh")
}

fn load_elf_image_from_fs(
    path: &str,
    argv0: &str,
) -> Result<crate::elf::LoadedImage, &'static str> {
    if !crate::fs::is_ready() {
        return Err("no filesystem");
    }
    let cwd = crate::fs::path::cwd_inode();
    let ino = crate::fs::ext2::resolve_path(cwd, path)?;
    crate::elf::load_from_ino(ino, &[argv0])
}

/// Load ELF64 from ext2 path (or embedded `hello` if path empty / "hello").
pub fn run_path(path: &str) -> Result<(), &'static str> {
    let path = path.trim();
    if path.is_empty() || path == "hello" || path == "/bin/hello" || path == "bin/hello" {
        // Prefer disk if present and file exists; else embedded.
        if crate::fs::is_ready() {
            if let Ok(()) = run_elf_from_fs("/bin/hello") {
                return Ok(());
            }
            if let Ok(()) = run_elf_from_fs("bin/hello") {
                return Ok(());
            }
        }
        return run_embedded_hello();
    }
    run_elf_from_fs(path)
}

fn run_elf_from_fs(path: &str) -> Result<(), &'static str> {
    if !crate::fs::is_ready() {
        return Err("no filesystem");
    }
    let cwd = crate::fs::path::cwd_inode();
    let ino = crate::fs::ext2::resolve_path(cwd, path)?;
    let argv0 = path.rsplit('/').next().unwrap_or(path);
    let image = crate::elf::load_from_ino(ino, &[argv0])?;
    enter_and_wait(
        image.entry,
        image.stack_top,
        image.brk_start,
        "exec: ELF64",
    )
}

/// Rust-callable from C asm (TSS rsp0).
#[no_mangle]
pub extern "C" fn tss_set_kernel_stack(rsp: u64) {
    tss::set_kernel_stack(rsp);
}
