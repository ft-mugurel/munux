//! Shell command implementations.

use core::arch::asm;

use crate::console;
use crate::gdt;
use crate::interrupts;
use crate::memory::{
    free_frames, heap_alloc_count, heap_end, heap_start, heap_used_bytes, kfree, kmalloc, ksize,
    page_directory_phys, total_frames, used_frames, virt_to_phys, KERNEL_HEAP_MAX,
};
use crate::x86::io::{outb, outw};

pub fn dispatch(line: &str) {
    let (cmd, rest) = match line.find(char::is_whitespace) {
        Some(i) => (&line[..i], line[i..].trim_start()),
        None => (line, ""),
    };

    match cmd {
        "help" | "?" => cmd_help(),
        "clear" | "cls" => {
            console::clear();
        }
        "about" | "version" => cmd_about(),
        "pmm" | "frames" => cmd_pmm(rest),
        "heap" | "kmalloc" => cmd_heap(rest),
        "ticks" | "time" => {
            console::print("ticks=");
            console::write_u64(interrupts::ticks() as u64);
            console::println("");
        }
        "idt" => {
            console::print("IDT present gates: ");
            console::write_u64(interrupts::present_gate_count() as u64);
            console::println("");
        }
        "gdt" => {
            console::print("GDT entries: ");
            console::write_u64(gdt::entry_count() as u64);
            console::println(" (null, kcode, kdata, tss-low, tss-high)");
        }
        "cr3" | "vmm" => {
            let cr3 = page_directory_phys().map(|p| p.as_u64()).unwrap_or(0);
            console::print("CR3=");
            console::write_hex64(cr3);
            console::println("");
            // sample identity check
            match virt_to_phys(0x100000) {
                Some(p) => {
                    console::print("virt_to_phys(1MiB)=");
                    console::write_hex64(p);
                    console::println("");
                }
                None => console::println("virt_to_phys(1MiB)=unmapped"),
            }
        }
        "echo" => {
            console::println(rest);
        }
        "reboot" => cmd_reboot(),
        "halt" | "shutdown" => cmd_halt(),
        "panic" => {
            panic!("shell panic command");
        }
        "fault" => {
            // optional deliberate #UD for testing
            if rest == "ud2" || rest.is_empty() {
                unsafe {
                    asm!("ud2", options(nomem, nostack));
                }
            } else {
                console::println("usage: fault [ud2]");
            }
        }
        other => {
            console::print("unknown command: `");
            console::print(other);
            console::println("` (try help)");
        }
    }
}

fn cmd_help() {
    console::println("munux shell commands:");
    console::println("  help            This list");
    console::println("  about           Kernel summary");
    console::println("  clear           Clear screen");
    console::println("  echo <text>     Print text");
    console::println("  pmm [test]      Physical frames");
    console::println("  heap [test]     Kernel heap / kmalloc");
    console::println("  ticks           PIT tick counter");
    console::println("  idt / gdt       Descriptor tables");
    console::println("  cr3 / vmm       Paging info");
    console::println("  reboot / halt   Machine control");
    console::println("  fault [ud2]     Trigger CPU exception");
    console::println("  panic           Rust panic");
}

fn cmd_about() {
    console::println("munux — freestanding x86_64 kernel (Rust + NASM)");
    console::println("PR1 long mode | PR2 GDT/IDT | PR3 PMM/paging");
    console::println("PR4 IRQs | PR5 heap + shell");
    console::print("PMM total=");
    console::write_u64(total_frames() as u64);
    console::print(" free=");
    console::write_u64(free_frames() as u64);
    console::println("");
    console::print("heap VA ");
    console::write_hex64(heap_start());
    console::print(" .. max ");
    console::write_hex64(KERNEL_HEAP_MAX);
    console::println("");
    console::print("ticks=");
    console::write_u64(interrupts::ticks() as u64);
    console::println("");
}

fn cmd_pmm(rest: &str) {
    let sub = rest.split_whitespace().next().unwrap_or("");
    console::print("frames total=");
    console::write_u64(total_frames() as u64);
    console::print(" used=");
    console::write_u64(used_frames() as u64);
    console::print(" free=");
    console::write_u64(free_frames() as u64);
    console::println("");
    if sub == "test" {
        match crate::memory::alloc_frame() {
            Some(f) => {
                console::print("alloc ");
                console::write_hex64(f.as_u64());
                console::println(" OK");
                crate::memory::free_frame(f);
                console::println("free OK");
            }
            None => console::println("alloc FAIL"),
        }
    }
}

fn cmd_heap(rest: &str) {
    let sub = rest.split_whitespace().next().unwrap_or("");
    console::print("heap used_bytes=");
    console::write_u64(heap_used_bytes() as u64);
    console::print(" allocs=");
    console::write_u64(heap_alloc_count() as u64);
    console::print(" end=");
    console::write_hex64(heap_end());
    console::println("");
    if sub == "test" {
        match kmalloc(64) {
            Some(p) => {
                console::print("kmalloc(64) -> ");
                console::write_hex64(p as u64);
                console::print(" ksize=");
                console::write_u64(ksize(p).unwrap_or(0) as u64);
                console::println("");
                unsafe {
                    *p = 0xA5;
                }
                kfree(p);
                console::println("kfree OK");
            }
            None => console::println("kmalloc FAIL"),
        }
    } else if sub == "help" || sub == "?" {
        console::println("heap [test]  — stats; test runs kmalloc/kfree");
    }
}

fn cmd_reboot() -> ! {
    console::println("reboot...");
    unsafe {
        outb(0x64, 0xFE); // keyboard controller pulse
        outw(0x604, 0x2000); // QEMU fallback
    }
    loop {
        unsafe {
            asm!("cli; hlt");
        }
    }
}

fn cmd_halt() -> ! {
    console::println("halt.");
    loop {
        unsafe {
            asm!("cli; hlt");
        }
    }
}
