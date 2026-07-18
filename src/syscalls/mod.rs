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
    pub const FORK: u64 = 57; // planned
    pub const EXECVE: u64 = 59; // planned
    pub const EXIT: u64 = 60;
    pub const WAIT4: u64 = 61; // planned
    pub const GETCWD: u64 = 79;
    pub const CHDIR: u64 = 80;
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

extern "C" {
    fn enter_user_mode(entry: u64, user_rsp: u64);
    fn return_from_user() -> !;
    fn set_syscall_kstack(rsp: u64);
    fn syscall_entry();
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
        num::GETPID => 1,
        num::GETCWD => sys_getcwd(a1, a2),
        num::CHDIR => sys_chdir(a1),
        num::GETDENTS64 => sys_getdents64(a1, a2, a3),
        num::EXIT | num::EXIT_GROUP => {
            let _status = a1;
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
    // Demo blob, classic ELF load (0x400000+), user stack (~0x7fff…), low identity
    (buf >= DEMO_CODE && end <= DEMO_STACK_TOP + 0x1000)
        || (buf >= 0x400000 && end <= 0x800000)
        || (buf >= 0x0000_0000_7000_0000 && end <= 0x0000_0000_8000_0000)
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

fn enter_and_wait(entry: u64, stack_top: u64, label: &str) {
    tss::set_kernel_stack(tss::kernel_stack_top());
    unsafe {
        set_syscall_kstack(tss::kernel_stack_top());
    }

    console::print(label);
    console::print(" entry=");
    console::write_hex64(entry);
    console::print(" stack=");
    console::write_hex64(stack_top);
    console::println("");

    unsafe {
        enter_user_mode(entry, stack_top);
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
    console::println("user: returned to kernel (exit)");
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
