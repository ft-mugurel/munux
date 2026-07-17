//! System calls via `syscall` / `sysret` and ring-3 demo.

use core::arch::asm;

use crate::console;
use crate::gdt::{self, STAR_KERNEL_CS, STAR_USER_BASE, USER_CODE_SELECTOR, USER_DATA_SELECTOR};
use crate::gdt::tss;
use crate::memory::paging::{self, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::memory::pmm::{self, FRAME_SIZE, PhysAddr};

pub mod num {
    pub const EXIT: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const GETPID: u64 = 5;
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
    console::println("syscall: STAR/LSTAR armed (SCE)");
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
        num::EXIT => {
            // Jump back to shell; does not return here.
            unsafe {
                return_from_user();
            }
        }
        num::WRITE => sys_write(a1, a2, a3),
        num::GETPID => 1,
        _ => u64::MAX,
    }
}

fn user_ptr_ok(buf: u64, len: u64) -> bool {
    if len > 0x10000 {
        return false;
    }
    let end = buf.saturating_add(len);
    // Demo blob, classic ELF load (0x400000+), user stack (~0x7fff…), low identity
    (buf >= DEMO_CODE && end <= DEMO_STACK_TOP + 0x1000)
        || (buf >= 0x400000 && end <= 0x800000)
        || (buf >= 0x0000_0000_7000_0000 && end <= 0x0000_0000_8000_0000)
        || (buf >= 0x1000 && end <= 0x4000_0000)
}

fn sys_write(fd: u64, buf: u64, len: u64) -> u64 {
    if fd != 1 && fd != 2 {
        return u64::MAX;
    }
    let len = len.min(4096);
    if !user_ptr_ok(buf, len) {
        return u64::MAX;
    }
    let slice = unsafe { core::slice::from_raw_parts(buf as *const u8, len as usize) };
    for &b in slice {
        if b == b'\n' {
            console::put_char(b'\n');
        } else if (32..127).contains(&b) || b == b'\t' {
            console::put_char(b);
        }
    }
    len
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
    // mov rax, 1  (WRITE)
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
    // mov rax, 0 (EXIT)
    out[i] = 0x48;
    out[i + 1] = 0xC7;
    out[i + 2] = 0xC0;
    out[i + 3..i + 7].copy_from_slice(&0u32.to_le_bytes());
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
