#![no_std]
#![no_main]

pub mod x86;
pub mod interrupts;
pub mod vga;
pub mod gdt;
pub mod shell;
pub mod panic;
pub mod memory;
pub mod process;
pub mod drivers;
pub mod fs;
pub mod elf;
pub mod syscalls;

use core::panic::PanicInfo;

use gdt::gdt::load_gdt;
use gdt::tss::init_tss;
use memory::{init_heap, init_pmm, init_paging};
use process::init_processes;
use syscalls::init_syscalls;

use interrupts::exceptions::init_exceptions;
use interrupts::keyboard::init::init_keyboard;
use interrupts::idt::init_idt;
use interrupts::pic::init_pic;
use interrupts::signal::{init_default_handlers, process_signals};
use interrupts::timer::init_timer;
use interrupts::utils::enable_interrupts;
use vga::text_mod::out::init_virtual_screens;

use crate::vga::text_mod::cursor::set_big_cursor;

extern "C" {
    static multiboot_magic_value: u32;
    static multiboot_info_addr: u32;
}

#[panic_handler]
fn rust_panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;

    struct Buf {
        data: [u8; 256],
        len: usize,
    }

    impl Buf {
        fn new() -> Self {
            Self {
                data: [0; 256],
                len: 0,
            }
        }

        fn as_str(&self) -> &str {
            core::str::from_utf8(&self.data[..self.len]).unwrap_or("panic")
        }
    }

    impl Write for Buf {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for &b in s.as_bytes() {
                if self.len < self.data.len() {
                    self.data[self.len] = b;
                    self.len += 1;
                }
            }
            Ok(())
        }
    }

    let mut buf = Buf::new();
    let _ = write!(&mut buf, "{}", info);
    panic::kernel_panic(buf.as_str());
}

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    let magic = unsafe { core::ptr::addr_of!(multiboot_magic_value).read_unaligned() };
    let mbi = unsafe { core::ptr::addr_of!(multiboot_info_addr).read_unaligned() };

    load_gdt();
    init_tss(); // GDT[7] + ltr — needed before ring-3 / int 0x80 stack switch
    init_idt();
    init_exceptions();
    unsafe {
        init_pic();
    }
    init_keyboard();
    init_timer(); // IRQ0 — CPU ticks for process signals
    init_default_handlers();
    set_big_cursor();
    enable_interrupts();
    init_virtual_screens();

    crate::println!("KFS i686 kernel");
    crate::println!(
        "IDT: {} gates | timer: on | signals: on",
        interrupts::present_gate_count()
    );

    init_pmm(magic, mbi);
    init_paging();
    init_heap();
    init_processes();
    fs::init();
    init_syscalls(); // int 0x80 DPL=3 (after paging so user pages can be mapped)

    shell::init();

    loop {
        process_signals();
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
