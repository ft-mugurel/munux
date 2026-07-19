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
    pub const UNAME: u64 = 63;
    pub const GETDENTS64: u64 = 217;
    pub const ARCH_PRCTL: u64 = 158; // planned (musl TLS)
    pub const BRK: u64 = 12;
    pub const MMAP: u64 = 9;
    pub const MUNMAP: u64 = 11;
    pub const SET_TID_ADDRESS: u64 = 218; // musl crt TLS/thread exit hook
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
///
/// ELF file bytes must **not** live on these stacks (see `ELF_LOAD_BUF`).
/// A former 64 KiB stack buffer on a 16 KiB nest stack overflowed into kernel
/// `.data` after the first `execve`.
const NEST_KSTACK_BYTES: usize = 32 * 1024;
const NEST_KSTACK_MAX: usize = 6;

/// Scratch for loading small ELFs from ext2 (off the nest stack).
/// Cooperative kernel: one load at a time.
const ELF_LOAD_CAP: usize = 64 * 1024;
static mut ELF_LOAD_BUF: [u8; ELF_LOAD_CAP] = [0; ELF_LOAD_CAP];

fn elf_load_buf() -> &'static mut [u8; ELF_LOAD_CAP] {
    // SAFETY: only called from the single-threaded syscall path; no concurrent load.
    unsafe { &mut *core::ptr::addr_of_mut!(ELF_LOAD_BUF) }
}

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
    // Ensure this process's TLS bases are in the CPU before ring 3.
    crate::process::apply_tls();
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
    a4: u64,
    a5: u64,
) -> u64 {
    // Never run kernel code with a user TLS base in FS/GS. User SET_FS stays
    // only on the PCB until we restore it for sysret (see end of this fn).
    crate::x86::msr::set_fs_base(0);
    crate::x86::msr::set_gs_base(0);

    let ret = match num {
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
        num::UNAME => sys_uname(a1),
        num::ARCH_PRCTL => sys_arch_prctl(a1, a2),
        num::BRK => sys_brk(a1),
        // mmap(addr, len, prot, flags, fd) — offset (6th arg) not passed by
        // our entry stub; anonymous maps require offset 0 (passed as 0 here).
        num::MMAP => sys_mmap(a1, a2, a3, a4, a5, 0),
        num::MUNMAP => sys_munmap(a1, a2),
        num::SET_TID_ADDRESS => sys_set_tid_address(a1),
        num::EXIT | num::EXIT_GROUP => {
            let status = a1 as i32;
            // Clear dying process TLS before switching to parent.
            crate::process::clear_tls();
            crate::process::exit_user(status);
            // Parent is current; load its TLS then leave ring 0 nest.
            crate::process::apply_tls();
            unsafe {
                return_from_user();
            }
        }
        _ => {
            // Always log so musl/static binary bring-up is not blind.
            console::print("syscall: ENOSYS n=");
            console::write_u64(num);
            console::println(" (-38)");
            errno::neg(errno::ENOSYS)
        }
    };

    // sysret path: put this process's TLS bases back in the CPU.
    crate::process::apply_tls();
    ret
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

/// Linux mmap(2) — anonymous private maps only for now.
fn sys_mmap(addr: u64, length: u64, prot: u64, flags: u64, fd: u64, offset: u64) -> u64 {
    match crate::process::proc_mmap(addr, length, prot, flags, fd, offset) {
        Ok(va) => va,
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
/// Musl calls this during crt init. We do not yet clear `*tidptr` on exit
/// (no robust futex waiters); returning the process id is enough for single-
/// threaded static binaries.
fn sys_set_tid_address(_tidptr: u64) -> u64 {
    // Optionally validate user pointer later; musl always passes a valid TLS slot.
    crate::process::getpid() as u64
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
    // Use addr_of_mut! so we never form a Rust reference to the mutable static.
    let mut len = 0usize;
    unsafe {
        let snap = core::ptr::addr_of_mut!(USER_IMAGE_SNAP).cast::<u8>();
        for off in (0..USER_IMAGE_MAX).step_by(FRAME_SIZE) {
            let v = USER_IMAGE_BASE + off as u64;
            if paging::virt_to_phys(v).is_none() {
                break;
            }
            let n = (USER_IMAGE_MAX - off).min(FRAME_SIZE);
            core::ptr::copy_nonoverlapping(v as *const u8, snap.add(off), n);
            len = off + n;
        }
        core::ptr::write(core::ptr::addr_of_mut!(USER_IMAGE_SNAP_LEN), len);
    }
}

fn restore_user_image() {
    unsafe {
        let len = core::ptr::read(core::ptr::addr_of!(USER_IMAGE_SNAP_LEN));
        if len == 0 {
            return;
        }
        let snap = core::ptr::addr_of!(USER_IMAGE_SNAP).cast::<u8>();
        // Ensure pages exist and are user-writable, then restore bytes.
        let mut off = 0usize;
        while off < len {
            let v = USER_IMAGE_BASE + off as u64;
            let _ = map_user_page(v);
            let n = (len - off).min(FRAME_SIZE);
            core::ptr::copy_nonoverlapping(snap.add(off), v as *mut u8, n);
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

/// Linux execve(path, argv, envp) — envp ignored; argv up to 3 user strings.
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

    // Collect argv strings (max 3) into kernel buffers.
    let mut a0 = [0u8; 64];
    let mut a1 = [0u8; 64];
    let mut a2 = [0u8; 64];
    let mut arg_lens = [0usize; 3];

    let argv0_default = path.rsplit('/').next().unwrap_or(path);
    // Default argv[0] from path basename (overwritten if user argv provided)
    let dlen = core::cmp::min(argv0_default.len(), 63);
    a0[..dlen].copy_from_slice(argv0_default.as_bytes());
    arg_lens[0] = dlen;
    let mut argc = 1usize;

    if argv_ptr != 0 && user_ptr_ok(argv_ptr, 8) {
        // argv is char**; read pointers until NULL (max 3)
        let mut n = 0usize;
        for i in 0..3u64 {
            let p = unsafe { core::ptr::read_volatile((argv_ptr + i * 8) as *const u64) };
            if p == 0 {
                break;
            }
            let slot = match i as usize {
                0 => &mut a0[..],
                1 => &mut a1[..],
                _ => &mut a2[..],
            };
            match copy_user_path(p, slot) {
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

    let s0 = core::str::from_utf8(&a0[..arg_lens[0]]).unwrap_or("?");
    let s1 = core::str::from_utf8(&a1[..arg_lens[1]]).unwrap_or("");
    let s2 = core::str::from_utf8(&a2[..arg_lens[2]]).unwrap_or("");
    let argv_refs: [&str; 3] = [s0, s1, s2];
    let argv_slice = &argv_refs[..argc];

    let image = match load_exec_image(path, argv_slice) {
        Ok(img) => img,
        Err("no filesystem") | Err("not found") | Err("ENOENT") => {
            return errno::neg(errno::ENOENT);
        }
        Err("is a directory") => return errno::neg(errno::EISDIR),
        Err("OOM") | Err("elf: OOM page") => return errno::neg(errno::ENOMEM),
        Err(_) => return errno::neg(errno::ENOENT),
    };

    // New image: drop old anonymous maps before replacing metadata.
    crate::process::clear_mmaps();

    let _ = crate::process::with_current(|p| {
        p.set_name(s0);
        p.user_rip = image.entry;
        p.user_rsp = image.stack_top;
        p.user_rax = 0;
        // New image: musl will re-set TLS; do not inherit previous FS base.
        p.fs_base = 0;
        p.gs_base = 0;
        // Fresh heap from ELF image end (Linux start_brk).
        p.heap_base = image.brk_start;
        p.heap_size = 0;
    });
    crate::x86::msr::set_fs_base(0);
    crate::x86::msr::set_gs_base(0);

    // Nested enter: new image runs until exit, then we unwind this session.
    enter_user_nested(image.entry, image.stack_top, 0);
    // New image exited: exit_user already switched to parent. Finish the
    // outer user session (wait4 child / shell task) — do not sysret.
    unsafe {
        return_from_user();
    }
}

fn load_exec_image(path: &str, argv: &[&str]) -> Result<crate::elf::LoadedImage, &'static str> {
    let argv0 = if argv.is_empty() { "?" } else { argv[0] };
    // Prefer filesystem; fall back to embedded known binaries.
    if crate::fs::is_ready() {
        if let Ok(img) = load_elf_from_fs(path, argv) {
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
                    if let Ok(img) = load_elf_from_fs(s, argv) {
                        return Ok(img);
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
    let _ = argv0;
    Err("ENOENT")
}

fn load_elf_from_fs(path: &str, argv: &[&str]) -> Result<crate::elf::LoadedImage, &'static str> {
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
    if size > ELF_LOAD_CAP {
        return Err("ELF too large");
    }
    let buf = elf_load_buf();
    let n = crate::fs::ext2::read_file(ino, 0, &mut buf[..size])?;
    crate::elf::load_bytes_argv(&buf[..n], argv)
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

fn enter_and_wait(entry: u64, stack_top: u64, brk_start: u64, label: &str) {
    enter_and_wait_opts(entry, stack_top, brk_start, label, false);
}

/// `quiet`: suppress pid/entry chatter (used for clean U8 boot handoff).
fn enter_and_wait_opts(entry: u64, stack_top: u64, brk_start: u64, label: &str, quiet: bool) {
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
}

/// Run the built-in hand-assembled ring-3 demo until exit.
pub fn run_demo_user_program() -> Result<(), &'static str> {
    setup_demo_image()?;
    // Demo blob is tiny; heap starts just after the demo page.
    enter_and_wait(DEMO_CODE, DEMO_STACK_TOP, DEMO_STACK_PAGE, "user: demo");
    Ok(())
}

/// Load an ELF64 image from bytes and run until exit.
pub fn exec_elf_bytes(file: &[u8], argv0: &str) -> Result<(), &'static str> {
    let image = crate::elf::load_bytes(file, argv0)?;
    enter_and_wait(
        image.entry,
        image.stack_top,
        image.brk_start,
        "exec: ELF64",
    );
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
    enter_and_wait_opts(image.entry, image.stack_top, image.brk_start, "sh", true);
    Ok(())
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
    if crate::fs::ext2::inode_is_dir(ino) {
        return Err("is a directory");
    }
    let size = crate::fs::ext2::inode_file_size(ino) as usize;
    if size == 0 || size > 512 * 1024 {
        return Err("bad file size");
    }
    if size > ELF_LOAD_CAP {
        return Err("ELF too large (max 64KiB)");
    }
    let buf = elf_load_buf();
    let n = crate::fs::ext2::read_file(ino, 0, &mut buf[..size])?;
    crate::elf::load_bytes(&buf[..n], argv0)
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
    if size > ELF_LOAD_CAP {
        return Err("ELF too large (max 64KiB)");
    }
    let buf = elf_load_buf();
    let n = crate::fs::ext2::read_file(ino, 0, &mut buf[..size])?;
    exec_elf_bytes(&buf[..n], path)
}

/// Rust-callable from C asm (TSS rsp0).
#[no_mangle]
pub extern "C" fn tss_set_kernel_stack(rsp: u64) {
    tss::set_kernel_stack(rsp);
}
