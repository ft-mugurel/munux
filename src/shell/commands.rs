//! Shell command implementations.

use core::arch::asm;

use crate::console;
use crate::fs;
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
            console::println(" (null,kcode,kdata,udata,ucode,tss x2)");
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
            if rest == "ud2" || rest.is_empty() {
                unsafe {
                    asm!("ud2", options(nomem, nostack));
                }
            } else {
                console::println("usage: fault [ud2]");
            }
        }
        "user" | "usermode" => {
            match crate::syscalls::run_demo_user_program() {
                Ok(()) => {}
                Err(e) => {
                    console::print("user: failed: ");
                    console::println(e);
                }
            }
        }
        "run" | "exec" | "hello" => {
            let path = rest.split_whitespace().next().unwrap_or("hello");
            if path == "help" || path == "?" {
                console::println("run [path|hello|echo]  — ELF64 from disk or embedded");
                console::println("  echo = U2 read/write test (type then Enter)");
                return;
            }
            if path == "echo" {
                // Preload stdin so automated tests / first keystroke path is reliable.
                // Interactive use: omit preload by using `run echoi` or type during read>
                crate::interrupts::keyboard::init::inject_str(b"hi\n");
                match crate::syscalls::run_embedded_echo() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run echo: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "echoi" {
                match crate::syscalls::run_embedded_echo() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run echoi: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "cat" {
                // Userland cat hello.txt via open/read/write/close
                match crate::syscalls::run_embedded_cat() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run cat: ");
                        console::println(e);
                    }
                }
                return;
            }
            match crate::syscalls::run_path(path) {
                Ok(()) => {}
                Err(e) => {
                    console::print("run: failed: ");
                    console::println(e);
                }
            }
        }
        "ls" => cmd_ls(rest),
        "cat" => cmd_cat(rest),
        "pwd" => cmd_pwd(),
        "cd" => cmd_cd(rest),
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
    console::println("  user            Enter ring 3 hand-asm demo");
    console::println("  run [path|echo|cat]  ELF64; cat=open/read file");
    console::println("  ls [path]       List directory");
    console::println("  cat <path>      Print file");
    console::println("  pwd / cd        Working directory");
}

fn cmd_about() {
    console::println("munux — freestanding x86_64 kernel (Rust + NASM)");
    console::println("PR1-8 boot..FS | U1 FD table (see docs/ABI.md)");
    console::print("FDs open=");
    console::write_u64(crate::fd::open_count() as u64);
    console::println(" (0=in 1=out 2=err)");
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

fn require_fs() -> bool {
    if !fs::is_ready() {
        console::println("fs: not mounted (need IDE disk.img)");
        return false;
    }
    true
}

fn cmd_ls(rest: &str) {
    if !require_fs() {
        return;
    }
    let path = rest.split_whitespace().next().unwrap_or(".");
    let cwd = fs::path::cwd_inode();
    let ino = match fs::ext2::resolve_path(cwd, path) {
        Ok(i) => i,
        Err(e) => {
            console::print("ls: ");
            console::println(e);
            return;
        }
    };
    if !fs::ext2::inode_is_dir(ino) {
        console::println("ls: not a directory");
        return;
    }
    match fs::ext2::list_dir(ino) {
        Ok(n) => {
            for i in 0..fs::vfs::cache_len() {
                if let Some(node) = fs::vfs::cache_get(i) {
                    let name = node.name_str();
                    if name == "." || name == ".." {
                        continue;
                    }
                    if node.kind == fs::vfs::NodeType::Directory {
                        console::print("d ");
                    } else {
                        console::print("- ");
                    }
                    console::print(name);
                    console::println("");
                }
            }
            let _ = n;
        }
        Err(e) => {
            console::print("ls: ");
            console::println(e);
        }
    }
}

fn cmd_cat(rest: &str) {
    if !require_fs() {
        return;
    }
    let path = rest.split_whitespace().next().unwrap_or("");
    if path.is_empty() {
        console::println("usage: cat <path>");
        return;
    }
    let cwd = fs::path::cwd_inode();
    let ino = match fs::ext2::resolve_path(cwd, path) {
        Ok(i) => i,
        Err(e) => {
            console::print("cat: ");
            console::println(e);
            return;
        }
    };
    if fs::ext2::inode_is_dir(ino) {
        console::println("cat: is a directory");
        return;
    }
    let mut buf = [0u8; 512];
    let mut off = 0u32;
    loop {
        match fs::ext2::read_file(ino, off, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                for &b in &buf[..n] {
                    if b == 0 {
                        break;
                    }
                    console::put_char(b);
                }
                off += n as u32;
            }
            Err(e) => {
                console::print("cat: ");
                console::println(e);
                break;
            }
        }
    }
    console::println("");
}

fn cmd_pwd() {
    if !require_fs() {
        return;
    }
    let mut out = [0u8; 128];
    let n = fs::path::getcwd_pretty(&mut out);
    if n > 0 {
        if let Ok(s) = core::str::from_utf8(&out[..n]) {
            console::println(s);
            return;
        }
    }
    console::println("/");
}

fn cmd_cd(rest: &str) {
    if !require_fs() {
        return;
    }
    let path = rest.split_whitespace().next().unwrap_or("/");
    match fs::path::chdir(path) {
        Ok(()) => {}
        Err(e) => {
            console::print("cd: ");
            console::println(e);
        }
    }
}
