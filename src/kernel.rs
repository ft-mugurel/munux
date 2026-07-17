//! munux kernel entry (x86_64).
//!
//! PR1: long mode + VGA banner
//! PR2: GDT64 + TSS64 + IDT64 + exception handlers

#![no_std]
#![no_main]

pub mod gdt;
pub mod interrupts;
pub mod vga_print;

use core::arch::asm;
use core::panic::PanicInfo;

use gdt::gdt::load_gdt;
use gdt::tss::init_tss;
use interrupts::exceptions::init_exceptions;
use interrupts::idt::{init_idt, present_gate_count};

/// Multiboot2 magic in EAX at entry (saved by trampoline).
const MULTIBOOT2_MAGIC: u32 = 0x36D7_6289;

extern "C" {
    static multiboot_magic_value: u32;
    static multiboot_info_addr: u32;
}

#[panic_handler]
fn rust_panic(info: &PanicInfo) -> ! {
    vga_print::clear_screen();
    vga_print::println_line(0, b"*** munux RUST PANIC ***", 0x4F);
    // Message is often not static; show a fixed hint.
    let _ = info;
    vga_print::println_line(2, b"(see exception path for CPU faults)", 0x0F);
    vga_print::println_line(22, b"System halted.", 0x08);
    loop {
        unsafe {
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    let magic = unsafe { core::ptr::addr_of!(multiboot_magic_value).read_unaligned() };
    let _mbi = unsafe { core::ptr::addr_of!(multiboot_info_addr).read_unaligned() };

    vga_print::clear_screen();
    vga_print::println_line(0, b"munux x86_64", 0x0F);
    vga_print::println_line(1, b"long mode OK", 0x0A);

    if magic == MULTIBOOT2_MAGIC {
        vga_print::println_line(2, b"multiboot2: OK", 0x0E);
    } else {
        vga_print::println_line(2, b"multiboot2: bad magic", 0x0C);
    }

    // --- PR2: descriptors + exceptions ---
    load_gdt();
    init_tss();
    init_idt();
    init_exceptions();

    let gates = present_gate_count();
    vga_print::print_str(4, 0, b"GDT+TSS: OK", 0x0A);
    vga_print::print_str(5, 0, b"IDT gates present: ", 0x07);
    vga_print::print_u64(5, 19, gates as u64, 0x0E);

    vga_print::println_line(7, b"Triggering #UD (ud2) to test handler...", 0x0B);

    // Deliberate invalid opcode -> vector 6 -> exception_handler panic screen.
    unsafe {
        asm!("ud2", options(nomem, nostack));
    }

    // Unreachable if IDT works.
    vga_print::println_line(9, b"ERROR: ud2 did not fault", 0x4F);
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}
