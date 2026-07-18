//! System calls via `syscall` / `sysret` and ring-3 demo.

use core::arch::asm;

use crate::console;
use crate::fd;
use crate::gdt::{self, STAR_KERNEL_CS, STAR_USER_BASE, USER_CODE_SELECTOR, USER_DATA_SELECTOR};
use crate::gdt::tss;
use crate::memory::paging::{self, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::memory::pmm::{self, FRAME_SIZE, PhysAddr};

/// Linux **x86_64** syscall numbers (see `arch/x86/entry/syscalls/syscall_64.tbl`).
/// Using Linux numbers is required so static Linux binaries can target munux later.
pub mod num {
    // Implemented / reserved with Linux numbers:
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const CLOSE: u64 = 3;
    pub const GETPID: u64 = 39;
    pub const FORK: u64 = 57;
    pub const EXECVE: u64 = 59;
    pub const EXIT: u64 = 60;
    pub const WAIT4: u64 = 61;
    pub const GETCWD: u64 = 79;
    pub const CHDIR: u64 = 80;
    pub const GETPPID: u64 = 110;
    pub const EXIT_GROUP: u64 = 231; // musl/glibc often use this
    pub const GETDENTS64: u64 = 217;
    pub const OPENAT: u64 = 257; // planned (modern libc)
}

/// Linux-style: return `-errno` as `u64` bit pattern (negative i64).
#[allow(dead_code)]
mod errno {
    pub const EPERM: i64 = 1;
    pub const ENOENT: i64 = 2;
    pub const EBADF: i64 = 9;
    pub const ECHILD: i64 = 10;
    pub const EAGAIN: i64 = 11;
    pub const ENOMEM: i64 = 12;
    pub const EFAULT: i64 = 14;
    pub const EISDIR: i64 = 21;
    pub const EINVAL: i64 = 22;
    pub const ENOTDIR: i64 = 20;
    pub const ENOSYS: i64 = 38;
    pub const ENAMETOOLONG: i64 = 36;
    pub const EMFILE: i64 = 24;
    pub const ERANGE: i64 = 34;

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
    }
}

/// User demo load addresses (outside 1 GiB identity map → mapped with U/S=1).
const DEMO_CODE: u64 = 0x0000_0000_4000_0000;
const DEMO_STACK_PAGE: u64 = 0x0000_0000_4000_1000;
const DEMO_STACK_TOP: u64 = DEMO_STACK_PAGE + 0x1000;

const PAGE_USER_RW: u64 = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

/// Nested syscall stacks so wait4/execve → child does not clobber the outer
/// syscall frame (all would otherwise share one `syscall_kstack` top).
const NEST_KSTACK_BYTES: usize = 16384;
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
    fn set_syscall_kstack(rsp: u64);
    fn syscall_entry();
    static last_user_rip: u64;
    static last_user_rsp: u64;
    static last_user_rflags: u64;
}

fn nest_stack_top(index: usize) -> u64 {
    unsafe {
        let base = core::ptr::addr_of!(NEST_KSTACKS[index]) as *const u8 as u64;
        base + NEST_KSTACK_BYTES as u64
    }
}

/// Push a fresh syscall/IRQ kernel stack for a nested user entry.
fn push_syscall_stack() {
    unsafe {
        if SYSCALL_STACK_DEPTH >= NEST_KSTACK_MAX {
            return;
        }
        SYSCALL_STACK_DEPTH += 1;
        let top = nest_stack_top(SYSCALL_STACK_DEPTH - 1);
        set_syscall_kstack(top);
        tss::set_kernel_stack(top);
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
            tss::set_kernel_stack(top);
        }
    }
}

fn ensure_kstack_base() {
    tss::set_kernel_stack(tss::kernel_stack_top());
    unsafe {
        set_syscall_kstack(tss::kernel_stack_top());
    }
}

/// Enter user with a private syscall stack for nested sessions.
fn enter_user_nested(entry: u64, user_rsp: u64, user_rax: u64) {
    push_syscall_stack();
    unsafe {
        enter_user_mode(entry, user_rsp, user_rax);
    }
    pop_syscall_stack();
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
    _a4: u64,
    _a5: u64,
) -> u64 {
    match num {
        num::READ => sys_read(a1, a2, a3),
        num::WRITE => sys_write(a1, a2, a3),
        num::OPEN => sys_open(a1, a2, a3),
        num::CLOSE => sys_close(a1),
        num::GETPID => crate::process::getpid() as u64,
        num::GETPPID => {
            let pp = crate::process::getppid();
            if pp < 0 {
                0
            } else {
                pp as u64
            }
        }
        num::FORK => sys_fork(),
        num::EXECVE => sys_execve(a1, a2, a3),
        num::WAIT4 => sys_wait4(a1, a2, a3),
        num::GETCWD => sys_getcwd(a1, a2),
        num::CHDIR => sys_chdir(a1),
        num::GETDENTS64 => sys_getdents64(a1, a2, a3),
        num::EXIT | num::EXIT_GROUP => {
            let status = a1 as i32;
            crate::process::exit_user(status);
            unsafe {
                return_from_user();
            }
        }
        _ => errno::neg(errno::ENOSYS),
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
    // Demo blob, classic ELF load (0x400000+), user stacks, fork child stacks
    (buf >= DEMO_CODE && end <= DEMO_STACK_TOP + 0x1000)
        || (buf >= 0x400000 && end <= 0x800000)
        || (buf >= 0x0000_0000_7000_0000 && end <= 0x0000_0000_8000_0000)
        || (buf >= 0x0000_0000_6F00_0000 && end <= 0x0000_0000_7000_0000)
        || (buf >= 0x1000 && end <= 0x4000_0000)
}

fn sys_write(fd: u64, buf: u64, len: u64) -> u64 {
    let len = len.min(4096);
    if !user_ptr_ok(buf, len) {
        return errno::neg(errno::EFAULT);
    }
    let slice = unsafe { core::slice::from_raw_parts(buf as *const u8, len as usize) };
    match fd::sys_write_slice(fd, slice) {
        Ok(n) => n as u64,
        Err(e) => map_fd_err(e),
    }
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

/// Linux open(path, flags, mode) — mode ignored; read-only files only.
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

/// Linux getdents64(fd, dirp, count) — bytes written, 0 at EOF, or -errno.
fn sys_getdents64(fd: u64, dirp: u64, count: u64) -> u64 {
    if count == 0 {
        return errno::neg(errno::EINVAL);
    }
    let count = count.min(4096) as usize;
    if !user_ptr_ok(dirp, count as u64) {
        return errno::neg(errno::EFAULT);
    }
    let mut tmp = [0u8; 4096];
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

// ENOTDIR used above
// add to errno module - I used ENOTDIR without defining it
fn copy_user_path(path_ptr: u64, out: &mut [u8]) -> Result<usize, u64> {
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
    if n == 0 {
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

/// Ensure a user-accessible page at `virt`.
fn map_user_page(virt: u64) -> Result<(), &'static str> {
    if virt & 0xFFF != 0 {
        return Err("unaligned");
    }
    // If already present, re-map with USER flags if needed
    if let Some(phys) = paging::virt_to_phys(virt) {
        let page = phys & !0xFFF;
        paging::map_page(virt, PhysAddr::new(page), PAGE_USER_RW);
        return Ok(());
    }
    let frame = pmm::alloc_frame().ok_or("oom")?;
    paging::map_page(virt, frame, PAGE_USER_RW);
    Ok(())
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

/// Snapshot of low user image (shared AS): restored after child may `execve`.
const USER_IMAGE_BASE: u64 = 0x400000;
const USER_IMAGE_MAX: usize = 64 * 1024;
static mut USER_IMAGE_SNAP: [u8; USER_IMAGE_MAX] = [0; USER_IMAGE_MAX];
static mut USER_IMAGE_SNAP_LEN: usize = 0;

fn snapshot_user_image() {
    // Copy present pages in [0x400000, 0x400000+64K)
    let mut len = 0usize;
    unsafe {
        for off in (0..USER_IMAGE_MAX).step_by(FRAME_SIZE) {
            let v = USER_IMAGE_BASE + off as u64;
            if paging::virt_to_phys(v).is_none() {
                break;
            }
            let n = (USER_IMAGE_MAX - off).min(FRAME_SIZE);
            core::ptr::copy_nonoverlapping(
                v as *const u8,
                USER_IMAGE_SNAP.as_mut_ptr().add(off),
                n,
            );
            len = off + n;
        }
        USER_IMAGE_SNAP_LEN = len;
    }
}

fn restore_user_image() {
    unsafe {
        let len = USER_IMAGE_SNAP_LEN;
        if len == 0 {
            return;
        }
        // Ensure pages exist and are user-writable, then restore bytes.
        let mut off = 0usize;
        while off < len {
            let v = USER_IMAGE_BASE + off as u64;
            let _ = map_user_page(v);
            let n = (len - off).min(FRAME_SIZE);
            core::ptr::copy_nonoverlapping(
                USER_IMAGE_SNAP.as_ptr().add(off),
                v as *mut u8,
                n,
            );
            off += n;
        }
    }
}

/// Run a Ready child to completion; restore parent user image afterward
/// (child `execve` would otherwise clobber shared code/data).
fn run_child_frame(frame: crate::process::UserFrame) {
    snapshot_user_image();
    enter_user_nested(frame.rip, frame.rsp, frame.rax);
    restore_user_image();
}

/// Linux fork() — parent returns child pid.
///
/// Cooperative: the Ready child is run to completion **before** the parent
/// resumes (avoids concurrent shared-AS mess). Parent then sees a zombie and
/// can `wait4` to reap.
fn sys_fork() -> u64 {
    let (rip, rsp, rflags) = unsafe {
        (
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rip)),
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rsp)),
            core::ptr::read_volatile(core::ptr::addr_of!(last_user_rflags)),
        )
    };
    let child_pid = match crate::process::fork_from_user(rip, rsp, rflags) {
        Ok(pid) => pid,
        Err(_) => return errno::neg(errno::EAGAIN),
    };

    // Run child now (nested enter). Child typically exits (or execve+exit).
    if let Some(frame) = crate::process::take_ready_child(child_pid) {
        run_child_frame(frame);
        // After exit: current is parent again; child is zombie.
    }

    child_pid as u64
}

/// Linux execve(path, argv, envp) — argv/envp ignored (argv0 = path basename).
/// On success does not return to the old image (nested enter + exit chain).
fn sys_execve(path_ptr: u64, _argv: u64, _envp: u64) -> u64 {
    let mut path_buf = [0u8; 256];
    let n = match copy_user_path(path_ptr, &mut path_buf) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let path = match core::str::from_utf8(&path_buf[..n]) {
        Ok(s) => s,
        Err(_) => return errno::neg(errno::ENOENT),
    };

    let argv0 = path.rsplit('/').next().unwrap_or(path);
    let image = match load_exec_image(path, argv0) {
        Ok(img) => img,
        Err("no filesystem") | Err("not found") | Err("ENOENT") => {
            return errno::neg(errno::ENOENT);
        }
        Err("is a directory") => return errno::neg(errno::EISDIR),
        Err("OOM") | Err("elf: OOM page") => return errno::neg(errno::ENOMEM),
        Err(_) => return errno::neg(errno::ENOENT),
    };

    let _ = crate::process::with_current(|p| {
        p.set_name(argv0);
        p.user_rip = image.entry;
        p.user_rsp = image.stack_top;
        p.user_rax = 0;
    });

    // Nested enter: new image runs until exit, then we unwind this session.
    enter_user_nested(image.entry, image.stack_top, 0);
    // New image exited: exit_user already switched to parent. Finish the
    // outer user session (wait4 child / shell task) — do not sysret.
    unsafe {
        return_from_user();
    }
}

fn load_exec_image(path: &str, argv0: &str) -> Result<crate::elf::LoadedImage, &'static str> {
    // Prefer filesystem; fall back to embedded known binaries.
    if crate::fs::is_ready() {
        if let Ok(img) = load_elf_from_fs(path, argv0) {
            return Ok(img);
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
                    if let Ok(img) = load_elf_from_fs(s, argv0) {
                        return Ok(img);
                    }
                }
            }
        }
    }
    // Embedded fallbacks
    if path == "hello"
        || path == "/bin/hello"
        || path == "bin/hello"
        || path.ends_with("/hello")
    {
        return crate::elf::load_bytes(crate::embedded_hello::HELLO_ELF, argv0);
    }
    if path == "echo" || path == "/bin/echo" || path.ends_with("/echo") {
        return crate::elf::load_bytes(crate::embedded_echo::ECHO_ELF, argv0);
    }
    if path == "cat" || path == "/bin/cat" || path.ends_with("/cat") {
        return crate::elf::load_bytes(crate::embedded_cat::CAT_ELF, argv0);
    }
    if path == "ls" || path == "/bin/ls" || path.ends_with("/ls") {
        return crate::elf::load_bytes(crate::embedded_ls::LS_ELF, argv0);
    }
    if path == "forktest" || path == "/bin/forktest" || path.ends_with("/forktest") {
        return crate::elf::load_bytes(crate::embedded_forktest::FORKTEST_ELF, argv0);
    }
    if path == "exectest" || path == "/bin/exectest" || path.ends_with("/exectest") {
        return crate::elf::load_bytes(crate::embedded_exectest::EXECTEST_ELF, argv0);
    }
    if path == "sh" || path == "/bin/sh" || path == "bin/sh" || path.ends_with("/sh") {
        return crate::elf::load_bytes(crate::embedded_sh::SH_ELF, argv0);
    }
    Err("ENOENT")
}

fn load_elf_from_fs(path: &str, argv0: &str) -> Result<crate::elf::LoadedImage, &'static str> {
    if !crate::fs::is_ready() {
        return Err("no filesystem");
    }
    let cwd = crate::fs::path::cwd_inode();
    let ino = crate::fs::ext2::resolve_path(cwd, path).map_err(|_| "not found")?;
    if crate::fs::ext2::inode_is_dir(ino) {
        return Err("is a directory");
    }
    let size = crate::fs::ext2::inode_file_size(ino) as usize;
    if size == 0 || size > 512 * 1024 {
        return Err("bad file size");
    }
    const CAP: usize = 64 * 1024;
    if size > CAP {
        return Err("ELF too large");
    }
    let mut buf = [0u8; CAP];
    let n = crate::fs::ext2::read_file(ino, 0, &mut buf[..size])?;
    crate::elf::load_bytes(&buf[..n], argv0)
}

/// Linux wait4(pid, status, options, rusage) — rusage ignored.
/// Reaps zombies. Also schedules any leftover Ready children (if fork did not
/// run them), then reaps.
fn sys_wait4(pid: u64, status_ptr: u64, options: u64) -> u64 {
    const WNOHANG: u64 = 1;
    let wait_for = pid as i32;
    let nohang = (options & WNOHANG) != 0;

    for _ in 0..16 {
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
        if let Some(frame) = crate::process::take_ready_child(wait_for) {
            run_child_frame(frame);
            continue;
        }
        return 0;
    }
    0
}

fn enter_and_wait(entry: u64, stack_top: u64, label: &str) {
    // U5/U6: run as a child of init (shell) so getpid/exit/wait/fork are real
    let child = match crate::process::begin_user_task(label) {
        Ok(p) => p,
        Err(_) => {
            console::println("user: process table full");
            return;
        }
    };

    // Base level (shell → first user task). Syscalls from that task use the
    // dedicated TSS/kernel stack (not a nest slot). Nested fork/exec children
    // get push_syscall_stack() so they do not clobber this frame.
    unsafe {
        SYSCALL_STACK_DEPTH = 0;
    }
    ensure_kstack_base();

    console::print(label);
    console::print(" pid=");
    console::write_u64(child as u64);
    console::print(" entry=");
    console::write_hex64(entry);
    console::print(" stack=");
    console::write_hex64(stack_top);
    console::println("");

    unsafe {
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
        console::print("user: exited pid=");
        console::write_u64(pid as u64);
        console::print(" status=");
        console::write_u64(code as u64);
        console::println("");
    } else {
        console::println("user: returned to kernel (no zombie?)");
    }
}

/// Run the built-in hand-assembled ring-3 demo until exit.
pub fn run_demo_user_program() -> Result<(), &'static str> {
    setup_demo_image()?;
    enter_and_wait(DEMO_CODE, DEMO_STACK_TOP, "user: demo");
    Ok(())
}

/// Load an ELF64 image from bytes and run until exit.
pub fn exec_elf_bytes(file: &[u8], argv0: &str) -> Result<(), &'static str> {
    let image = crate::elf::load_bytes(file, argv0)?;
    enter_and_wait(image.entry, image.stack_top, "exec: ELF64");
    Ok(())
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

/// Run embedded `exectest` (fork + execve + wait4) — U6 test.
pub fn run_embedded_exectest() -> Result<(), &'static str> {
    exec_elf_bytes(crate::embedded_exectest::EXECTEST_ELF, "exectest")
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
    if crate::fs::ext2::inode_is_dir(ino) {
        return Err("is a directory");
    }
    let size = crate::fs::ext2::inode_file_size(ino) as usize;
    if size == 0 || size > 512 * 1024 {
        return Err("bad file size");
    }
    // Stack buffer for small ELFs (hello is tiny). Cap 64 KiB.
    const CAP: usize = 64 * 1024;
    if size > CAP {
        return Err("ELF too large (max 64KiB)");
    }
    let mut buf = [0u8; CAP];
    let n = crate::fs::ext2::read_file(ino, 0, &mut buf[..size])?;
    exec_elf_bytes(&buf[..n], path)
}

/// Rust-callable from C asm (TSS rsp0).
#[no_mangle]
pub extern "C" fn tss_set_kernel_stack(rsp: u64) {
    tss::set_kernel_stack(rsp);
}
