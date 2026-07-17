//! System call interface (`int 0x80`), ELF exec, and built-in ring-3 demo.

use core::arch::asm;

use crate::elf::{self, LoadedImage};
use crate::gdt::tss;
use crate::interrupts::idt::{register_gate, GATE_INTERRUPT_USER};
use crate::memory::pmm::FRAME_SIZE;
use crate::process;

pub mod num {
    pub const EXIT: u32 = 0;
    pub const WRITE: u32 = 1;
    pub const READ: u32 = 2;
    pub const OPEN: u32 = 3;
    pub const CLOSE: u32 = 4;
    pub const GETPID: u32 = 5;
    pub const GETUID: u32 = 6;
    pub const FORK: u32 = 7;
    pub const WAIT: u32 = 8;
    pub const KILL: u32 = 9;
    pub const SIGNAL: u32 = 10;
    pub const EXEC: u32 = 11;
}

/// Built-in demo load address (hand-assembled, not ELF).
const DEMO_CODE_BASE: u32 = 0x0040_0000;
const DEMO_STACK_TOP: u32 = 0x0050_0000;
const DEMO_STACK_PAGE: u32 = DEMO_STACK_TOP - 0x1000;

extern "C" {
    fn isr_syscall();
    fn enter_user_mode(entry: u32, user_esp: u32);
    fn return_from_user() -> !;
}

/// Called from assembly with kernel ESP so TSS.esp0 is correct.
#[no_mangle]
pub extern "C" fn tss_set_esp0_from_esp(esp: u32) {
    tss::set_kernel_stack(esp);
}

pub fn init_syscalls() {
    register_gate(0x80, isr_syscall, GATE_INTERRUPT_USER);
    crate::println!("syscall: int 0x80 armed (DPL=3)");
}

/// C ABI entry from `isr_syscall`.
#[no_mangle]
pub extern "C" fn syscall_dispatch(
    num: u32,
    a1: u32,
    a2: u32,
    a3: u32,
    _a4: u32,
    _a5: u32,
) -> u32 {
    match num {
        num::EXIT => {
            unsafe {
                return_from_user();
            }
        }
        num::WRITE => sys_write(a1, a2, a3),
        num::READ => sys_read(a1, a2, a3),
        num::OPEN => sys_open(a1) as u32,
        num::CLOSE => 0,
        num::GETPID => process::current_pid() as u32,
        num::GETUID => process::getuid() as u32,
        num::FORK => process::fork() as u32,
        num::WAIT => {
            let mut st = 0i32;
            process::wait(Some(&mut st)) as u32
        }
        num::KILL => process::kill(a1 as i32, a2) as u32,
        num::SIGNAL => process::signal(a1, a2 as usize) as u32,
        // In-process exec of a path is kernel-only for now (shell `run`).
        // User-callable EXEC can be added once we have a path buffer ABI.
        num::EXEC => u32::MAX,
        _ => u32::MAX,
    }
}

fn user_ptr_ok(buf: u32, len: u32) -> bool {
    if buf < 0x1000 || len > 0x10000 {
        return false;
    }
    (buf as u64).saturating_add(len as u64) <= 0xC000_0000
}

fn sys_write(fd: u32, buf: u32, len: u32) -> u32 {
    if fd != 1 && fd != 2 {
        return u32::MAX;
    }
    let len = len.min(4096);
    if !user_ptr_ok(buf, len) {
        return u32::MAX;
    }
    let slice = unsafe { core::slice::from_raw_parts(buf as *const u8, len as usize) };
    for &b in slice {
        if b == b'\n' {
            crate::println!();
        } else if (32..127).contains(&b) || b == b'\t' {
            crate::print!("{}", b as char);
        }
    }
    len
}

fn sys_read(fd: u32, buf: u32, len: u32) -> u32 {
    let _ = (fd, buf, len);
    0
}

fn sys_open(path_ptr: u32) -> i32 {
    if !user_ptr_ok(path_ptr, 1) {
        return -1;
    }
    let mut path = [0u8; 128];
    let mut n = 0usize;
    unsafe {
        loop {
            let b = core::ptr::read_volatile((path_ptr as usize + n) as *const u8);
            if b == 0 || n + 1 >= path.len() {
                break;
            }
            path[n] = b;
            n += 1;
        }
    }
    let s = core::str::from_utf8(&path[..n]).unwrap_or("");
    if !crate::fs::is_ready() {
        return -1;
    }
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2::resolve_path(cwd, s) {
        Ok(ino) => ino as i32,
        Err(_) => -1,
    }
}

fn restore_kernel_after_user() {
    unsafe {
        asm!(
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "sti",
            options(nostack)
        );
    }
}

fn enter_and_wait(image: LoadedImage) {
    crate::println!(
        "exec: entry={:#x} stack={:#x}",
        image.entry,
        image.stack_top
    );
    unsafe {
        enter_user_mode(image.entry, image.stack_top);
    }
    restore_kernel_after_user();
    crate::println!("exec: process exited -> kernel");
}

/// Load ELF from path and run it in ring 3 until `exit`.
pub fn exec_path(path: &str) -> Result<(), &'static str> {
    let image = elf::load_path(path)?;
    enter_and_wait(image);
    Ok(())
}

/// Built-in hand-assembled demo (no filesystem required).
pub fn run_demo_user_program() -> Result<(), &'static str> {
    setup_demo_image()?;
    enter_and_wait(LoadedImage {
        entry: DEMO_CODE_BASE,
        stack_top: DEMO_STACK_TOP,
    });
    Ok(())
}

fn setup_demo_image() -> Result<(), &'static str> {
    elf::map_user_page(DEMO_CODE_BASE)?;
    elf::map_user_page(DEMO_STACK_PAGE)?;
    let prog = user_demo_bytes();
    unsafe {
        core::ptr::write_bytes(DEMO_CODE_BASE as *mut u8, 0, FRAME_SIZE);
        core::ptr::copy_nonoverlapping(prog.as_ptr(), DEMO_CODE_BASE as *mut u8, prog.len());
        core::ptr::write_bytes(DEMO_STACK_PAGE as *mut u8, 0, FRAME_SIZE);
    }
    Ok(())
}

fn user_demo_bytes() -> [u8; 256] {
    let mut out = [0u8; 256];
    let msg = b"Hello from ring 3 user mode via int 0x80!\n\0";
    out[0x80..0x80 + msg.len()].copy_from_slice(msg);
    let msg_addr = DEMO_CODE_BASE + 0x80;
    let msg_len = (msg.len() - 1) as u32;

    let mut i = 0usize;
    out[i] = 0xB8;
    out[i + 1..i + 5].copy_from_slice(&1u32.to_le_bytes());
    i += 5;
    out[i] = 0xBB;
    out[i + 1..i + 5].copy_from_slice(&1u32.to_le_bytes());
    i += 5;
    out[i] = 0xB9;
    out[i + 1..i + 5].copy_from_slice(&msg_addr.to_le_bytes());
    i += 5;
    out[i] = 0xBA;
    out[i + 1..i + 5].copy_from_slice(&msg_len.to_le_bytes());
    i += 5;
    out[i] = 0xCD;
    out[i + 1] = 0x80;
    i += 2;
    out[i] = 0xB8;
    out[i + 1..i + 5].copy_from_slice(&0u32.to_le_bytes());
    i += 5;
    out[i] = 0xBB;
    out[i + 1..i + 5].copy_from_slice(&0u32.to_le_bytes());
    i += 5;
    out[i] = 0xCD;
    out[i + 1] = 0x80;
    let _ = i;
    out
}
