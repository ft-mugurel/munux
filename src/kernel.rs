//! munux kernel entry (x86_64).
//!
//! Boot: Multiboot → long mode → GDT/TSS/IDT → PMM/paging/heap → processes →
//! syscalls → ext2 → userspace `/bin/sh` (kernel shell is debug fallback).

#![no_std]
#![no_main]

pub mod console;
pub mod tty;
pub mod drivers;
pub mod elf;
pub mod embedded_cat;
pub mod embedded_echo;
pub mod embedded_exectest;
pub mod embedded_forktest;
pub mod embedded_hello;
pub mod embedded_ls;
pub mod embedded_sh;
pub mod embedded_archprctl;
pub mod embedded_brktest;
pub mod embedded_mmaptest;
pub mod embedded_polltest;
pub mod embedded_p9test;
pub mod embedded_preempttest;
pub mod embedded_clonetest;
pub mod embedded_futextest;
pub mod embedded_signaltest;
pub mod embedded_uname;
pub mod embedded_vi;
pub mod fd;
pub mod fs;
pub mod gdt;
pub mod interrupts;
pub mod linuxkpi;
pub mod memory;
pub mod net;
pub mod module;
pub mod process;
pub mod shell;
pub mod syscalls;
pub mod vga;
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

    console::init(); // bitmap font + clear + hardware cursor
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

    init_timer();
    init_keyboard();
    unsafe {
        init_pic();
    }
    enable_interrupts();
    console::print("IRQs ON  IDT gates=");
    console::write_u64(present_gate_count() as u64);
    console::println("");
    linuxkpi::pci::init();

    fd::init();
    console::print("fd: stdio installed open=");
    console::write_u64(fd::open_count() as u64);
    console::println("");

    process::init_processes();
    init_syscalls();
    fs::init();
    module::init();

    console::set_color(0x0A);
    console::println("boot OK");
    console::set_color(0x07);

    // Diagnostics are done — clear for a clean userspace screen.
    console::clear();
    console::set_color(0x0F);
    console::println("munux");
    console::set_color(0x08);
    console::println("type exit in sh for kernel debug shell");
    console::set_color(0x07);
    console::println("");

    // Userspace init (/bin/sh). kinit remains pid 1; sh is a child.
    match syscalls::run_init_sh() {
        Ok(()) => {
            console::clear();
            console::set_color(0x0A);
            console::println("userspace exited — kernel debug shell");
            console::set_color(0x07);
        }
        Err(e) => {
            console::set_color(0x0C);
            console::print("init failed: ");
            console::println(e);
            console::set_color(0x07);
        }
    }

    shell::init();

    loop {
        shell::poll();
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}
