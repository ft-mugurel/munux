//! munux kernel entry (x86_64).
//!
//! PR1–PR4: boot, GDT/IDT, memory, IRQs
//! PR5: heap + interactive shell
//! PR6: syscall + ring-3 user demo
//! PR7: ELF64 loader + embedded hello
//! PR8: IDE + ext2 (read) + shell FS commands
//! U1: FD table + WRITE via FDs (`docs/ABI.md`)
//! U8: boot handoff to userspace `/bin/sh` (kernel shell is debug fallback)

#![no_std]
#![no_main]

pub mod console;
pub mod drivers;
pub mod elf;
pub mod embedded_cat;
pub mod embedded_echo;
pub mod embedded_exectest;
pub mod embedded_forktest;
pub mod embedded_hello;
pub mod embedded_ls;
pub mod embedded_sh;
pub mod fd;
pub mod fs;
pub mod gdt;
pub mod interrupts;
pub mod memory;
pub mod process;
pub mod shell;
pub mod syscalls;
pub mod vga_print;
pub mod x86;

use core::arch::asm;
use core::panic::PanicInfo;

use gdt::gdt::load_gdt;
use gdt::tss::init_tss;
use interrupts::exceptions::init_exceptions;
use interrupts::idt::{init_idt, present_gate_count};
use interrupts::{enable_interrupts, init_keyboard, init_pic, init_timer};
use memory::{
    free_frames, init_heap, init_paging, init_pmm, kmalloc, kfree, page_directory_phys,
    MULTIBOOT2_MAGIC,
};
use syscalls::init_syscalls;

extern "C" {
    static multiboot_magic_value: u32;
    static multiboot_info_addr: u32;
}

#[panic_handler]
fn rust_panic(_info: &PanicInfo) -> ! {
    // Best-effort: raw VGA (console may be mid-write)
    vga_print::clear_screen();
    vga_print::println_line(0, b"*** munux RUST PANIC ***", 0x4F);
    vga_print::println_line(2, b"System halted.", 0x08);
    loop {
        unsafe {
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    let magic = unsafe { core::ptr::addr_of!(multiboot_magic_value).read_unaligned() };
    let mbi = unsafe { core::ptr::addr_of!(multiboot_info_addr).read_unaligned() };

    console::clear();
    console::set_color(0x0F);
    console::println("munux x86_64");
    console::set_color(0x0A);
    console::println("long mode OK");
    console::set_color(0x07);

    if magic == MULTIBOOT2_MAGIC {
        console::println("multiboot2: OK");
    } else {
        console::set_color(0x0C);
        console::println("multiboot2: bad magic");
        console::set_color(0x07);
    }

    load_gdt();
    init_tss();
    init_idt();
    init_exceptions();
    console::print("GDT+TSS OK  IDT gates=");
    console::write_u64(present_gate_count() as u64);
    console::println("");

    init_pmm(magic, mbi);
    console::print("PMM free frames=");
    console::write_u64(free_frames() as u64);
    console::println("");

    init_paging();
    let cr3 = page_directory_phys().map(|p| p.as_u64()).unwrap_or(0);
    console::print("paging ON  CR3=");
    console::write_hex64(cr3);
    console::println("");

    // --- PR5: heap ---
    init_heap();
    if let Some(p) = kmalloc(32) {
        console::print("heap kmalloc OK @ ");
        console::write_hex64(p as u64);
        console::println("");
        kfree(p);
    } else {
        console::set_color(0x0C);
        console::println("heap kmalloc FAIL");
        console::set_color(0x07);
    }

    // --- PR4: IRQs ---
    init_timer();
    init_keyboard();
    unsafe {
        init_pic();
    }
    enable_interrupts();
    console::print("IRQs ON  IDT gates=");
    console::write_u64(present_gate_count() as u64);
    console::println("");

    // --- U1: file descriptors (stdio 0/1/2) ---
    fd::init();
    console::print("fd: stdio installed open=");
    console::write_u64(fd::open_count() as u64);
    console::println("");

    // --- U5: process table (init = pid 1 = shell) ---
    process::init_processes();

    // --- PR6: syscalls ---
    init_syscalls();

    // --- PR8: filesystem ---
    fs::init();

    // --- U8: userspace init (/bin/sh) ---
    // Kernel process table still has pid 1 = kinit (this idle context).
    // /bin/sh runs as a child until exit, then we fall back to munux> for debug.
    console::set_color(0x0E);
    console::println("U8: handoff → /bin/sh  (type exit for kernel shell)");
    console::set_color(0x07);
    match syscalls::run_init_sh() {
        Ok(()) => {
            console::set_color(0x0A);
            console::println("U8: /bin/sh exited — kernel debug shell");
            console::set_color(0x07);
        }
        Err(e) => {
            console::set_color(0x0C);
            console::print("U8: init failed: ");
            console::println(e);
            console::set_color(0x07);
        }
    }

    // --- PR5 / debug: kernel shell (also fallback when userspace init exits) ---
    shell::init();

    loop {
        shell::poll();
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}
