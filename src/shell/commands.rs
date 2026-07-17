//! Shell command implementations.

use core::arch::asm;

use crate::gdt::gdt::{
    entry_count, read_installed_entry, GDT_ADDRESS, KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR,
    KERNEL_STACK_SELECTOR, USER_CODE_SELECTOR, USER_DATA_SELECTOR, USER_STACK_SELECTOR,
};
use crate::vga::text_mod::out::{self, Color, ColorCode};
use crate::x86::io::{outb, outw};

const ENTRY_NAMES: [&str; 8] = [
    "null",
    "kernel_code",
    "kernel_data",
    "kernel_stack",
    "user_code",
    "user_data",
    "user_stack",
    "tss",
];

pub fn dispatch(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    let (cmd, rest) = match line.find(char::is_whitespace) {
        Some(i) => (&line[..i], line[i..].trim_start()),
        None => (line, ""),
    };

    match cmd {
        "help" | "?" => cmd_help(rest),
        "clear" | "cls" => cmd_clear(),
        "echo" => cmd_echo(rest),
        "gdt" => cmd_gdt(),
        "stack" => cmd_stack(rest),
        "regs" | "registers" => cmd_regs(),
        "idt" => cmd_idt(),
        "mem" | "peek" => cmd_mem(rest),
        "reboot" => cmd_reboot(),
        "halt" | "shutdown" => cmd_halt(),
        "about" | "version" => cmd_about(),
        "color" => cmd_color(rest),
        "panic" => cmd_panic(rest),
        "fault" => cmd_fault(rest),
        "pmm" | "frames" => cmd_pmm(rest),
        "vmm" | "paging" | "page" => cmd_vmm(rest),
        "heap" | "kmalloc" => cmd_heap(rest),
        "signal" | "sig" => cmd_signal(rest),
        "ps" | "proc" => cmd_ps(rest),
        "fork" => cmd_fork(rest),
        "wait" => cmd_wait(rest),
        "kill" => cmd_kill(rest),
        "getuid" => cmd_getuid(),
        "socket" => cmd_socket(rest),
        "cat" => cmd_cat(rest),
        "pwd" => cmd_pwd(rest),
        "cd" => cmd_cd(rest),
        "ls" => cmd_ls(rest),
        "mkdir" => cmd_mkdir(rest),
        "touch" => cmd_touch(rest),
        "rm" => cmd_rm(rest),
        "rmdir" => cmd_rmdir(rest),
        "user" | "usermode" | "syscall" => cmd_user(rest),
        "run" | "exec" => cmd_run(rest),
        other => {
            crate::println!("unknown command: `{}` (try `help`)", other);
        }
    }
}

/// Top-level help, or `help <command>` for a topic.
fn cmd_help(rest: &str) {
    let topic = rest.split_whitespace().next().unwrap_or("");
    if !topic.is_empty() {
        cmd_help_topic(topic);
        return;
    }

    crate::println!("KFS shell — commands:");
    crate::println!("  help [cmd]   This list, or help for one command");
    crate::println!("  about        Kernel / memory summary");
    crate::println!("  clear        Clear screen");
    crate::println!("  echo         Print text");
    crate::println!("  gdt          Dump GDT");
    crate::println!("  stack        Dump stack");
    crate::println!("  regs         CPU registers");
    crate::println!("  idt          IDT / IRQ summary");
    crate::println!("  mem          Hexdump memory");
    crate::println!("  color        Set text color");
    crate::println!("  pmm          Physical memory (frames)");
    crate::println!("  vmm          Virtual memory (paging)");
    crate::println!("  heap         Kernel heap (kmalloc)");
    crate::println!("  signal       Signal / callback system");
    crate::println!("  ps           List processes");
    crate::println!("  fork         Fork current process");
    crate::println!("  wait         Wait for zombie child");
    crate::println!("  kill         Queue signal to PID");
    crate::println!("  getuid       Current process uid");
    crate::println!("  socket       IPC sockets between processes");
    crate::println!("  ls           List directory");
    crate::println!("  cat          Print file contents");
    crate::println!("  pwd          Print working directory");
    crate::println!("  cd           Change directory");
    crate::println!("  mkdir        Create directory");
    crate::println!("  touch        Create empty file");
    crate::println!("  rm           Remove file");
    crate::println!("  rmdir        Remove empty directory");
    crate::println!("  user         Run ring-3 demo (int 0x80 syscalls)");
    crate::println!("  run <path>   Load ELF32 from disk and exec in ring 3");
    crate::println!("  panic        Intentional kernel panic");
    crate::println!("  fault        Trigger a CPU exception");
    crate::println!("  reboot       Reset machine");
    crate::println!("  halt         Freeze CPU");
    crate::println!();
    crate::println!("Type `<command> help` for subcommands (e.g. `heap help`).");
    crate::println!("Keys: F1-F6 screens | Shift+Up/Down scroll | Ctrl+Alt+Del poweroff");
}

fn cmd_help_topic(topic: &str) {
    match topic {
        "help" | "?" => {
            crate::println!("help [command]");
            crate::println!("  help           List top-level commands");
            crate::println!("  help <cmd>     Same as `<cmd> help`");
        }
        "about" | "version" => {
            crate::println!("about");
            crate::println!("  Print architecture, GDT selectors, PMM/paging/heap summary.");
        }
        "clear" | "cls" => {
            crate::println!("clear");
            crate::println!("  Clear the active virtual screen.");
        }
        "echo" => {
            crate::println!("echo <text>");
            crate::println!("  Print the rest of the line to the console.");
        }
        "gdt" => {
            crate::println!("gdt");
            crate::println!("  Dump the GDT installed at 0x{:08X}.", GDT_ADDRESS);
        }
        "stack" => help_stack(),
        "regs" | "registers" => {
            crate::println!("regs");
            crate::println!("  Dump general-purpose and segment registers.");
        }
        "idt" => help_idt(),
        "mem" | "peek" => help_mem(),
        "color" => help_color(),
        "pmm" | "frames" => help_pmm(),
        "vmm" | "paging" | "page" => help_vmm(),
        "heap" | "kmalloc" => help_heap(),
        "signal" | "sig" => help_signal(),
        "ps" | "proc" => help_ps(),
        "fork" => help_fork(),
        "wait" => help_wait(),
        "kill" => help_kill(),
        "getuid" => help_getuid(),
        "socket" => help_socket(),
        "ls" => help_ls(),
        "cat" => help_cat(),
        "pwd" => help_pwd(),
        "cd" => help_cd(),
        "mkdir" => help_mkdir(),
        "touch" => help_touch(),
        "rm" => help_rm(),
        "rmdir" => help_rmdir(),
        "user" | "usermode" | "syscall" => help_user(),
        "run" | "exec" => help_run(),
        "panic" => help_panic(),
        "fault" => help_fault(),
        "reboot" => {
            crate::println!("reboot");
            crate::println!("  Attempt reset via keyboard controller.");
        }
        "halt" | "shutdown" => {
            crate::println!("halt");
            crate::println!("  Disable interrupts and halt forever.");
        }
        other => {
            crate::println!("no help topic for `{}` (try `help`)", other);
        }
    }
}

fn help_user() {
    crate::println!("user — ring-3 demo via int 0x80");
    crate::println!("  user           Map a tiny program at 0x400000, enter CPL=3,");
    crate::println!("                 call write(1, msg, n) then exit(0), return to shell.");
    crate::println!("  Aliases: usermode, syscall");
    crate::println!();
    crate::println!("Syscalls (EAX): EXIT=0 WRITE=1 READ=2 OPEN=3 CLOSE=4");
    crate::println!("  GETPID=5 GETUID=6 FORK=7 WAIT=8 KILL=9 SIGNAL=10 EXEC=11");
    crate::println!("For real ELF binaries from disk, use: run /bin/hello");
}

fn help_run() {
    crate::println!("run <path> — load ELF32 ET_EXEC from ext2 and execute");
    crate::println!("  run /bin/hello     Typical demo binary on the disk image");
    crate::println!("  exec <path>        Alias of run");
    crate::println!();
    crate::println!("Requirements: static i386 ELF, PT_LOAD segments in user VA");
    crate::println!("  (< 0xC0000000). Syscalls via int 0x80 (EXIT=0, WRITE=1, …).");
    crate::println!("Returns to the shell when the program calls exit().");
}

fn cmd_user(rest: &str) {
    let sub = rest.split_whitespace().next().unwrap_or("");
    if sub == "help" || sub == "?" {
        help_user();
        return;
    }
    match crate::syscalls::run_demo_user_program() {
        Ok(()) => {}
        Err(e) => crate::println!("user: failed: {}", e),
    }
}

fn cmd_run(rest: &str) {
    let path = rest.split_whitespace().next().unwrap_or("");
    if path.is_empty() || path == "help" || path == "?" {
        help_run();
        return;
    }
    match crate::syscalls::exec_path(path) {
        Ok(()) => {}
        Err(e) => crate::println!("run: {}: {}", path, e),
    }
}

fn help_stack() {
    crate::println!("stack [n]");
    crate::println!("  Dump n dwords from ESP (default 16, max 64).");
}

fn help_mem() {
    crate::println!("mem <hex_addr> [nbytes]");
    crate::println!("  Hexdump nbytes at address (default 16, max 256).");
    crate::println!("  Alias: peek");
}

fn help_color() {
    crate::println!("color <name>");
    crate::println!("  Set foreground color.");
    crate::println!("  names: white green cyan yellow red magenta blue gray brown dark");
}

fn help_pmm() {
    crate::println!("pmm — physical memory manager (frames)");
    crate::println!("  pmm              Show frame stats");
    crate::println!("  pmm info         Same as pmm");
    crate::println!("  pmm alloc [n]    phys_alloc n bytes (default 4096)");
    crate::println!("  pmm frame        alloc_frame (one 4 KiB frame)");
    crate::println!("  pmm free <hex>   Free physical allocation / frame");
    crate::println!("  pmm size <hex>   Size of physical allocation");
    crate::println!("  pmm test         Alloc/free self-test");
    crate::println!("  pmm help         This help");
    crate::println!("  Alias: frames");
}

fn help_vmm() {
    crate::println!("vmm — virtual memory / paging");
    crate::println!("  vmm              Show paging status (CR0/CR3, spaces)");
    crate::println!("  vmm info         Same as vmm");
    crate::println!("  vmm page <virt>  Walk page tables for address");
    crate::println!("  vmm map <v> [p]  Map page (alloc frame if p omitted)");
    crate::println!("  vmm unmap <v>    Unmap page (does not free frame)");
    crate::println!("  vmm test         Map/unmap self-test");
    crate::println!("  vmm help         This help");
    crate::println!("  Aliases: paging, page");
}

fn help_heap() {
    crate::println!("heap — kernel heap (kmalloc over virtual pages)");
    crate::println!("  heap             Show heap stats");
    crate::println!("  heap info        Same as heap");
    crate::println!("  heap alloc [n]   kmalloc n bytes (default 64)");
    crate::println!("  heap free <ptr>  kfree pointer");
    crate::println!("  heap size <ptr>  ksize pointer");
    crate::println!("  heap test        Alloc/free self-test");
    crate::println!("  heap help        This help");
    crate::println!("  Alias: kmalloc");
}

fn help_panic() {
    crate::println!("panic [message]");
    crate::println!("  Trigger kernel_panic and halt (for testing).");
}

fn help_fault() {
    crate::println!("fault <kind>");
    crate::println!("  Trigger a CPU exception (for testing handlers).");
    crate::println!("  kinds:");
    crate::println!("    div      Divide-by-zero (#DE)");
    crate::println!("    gpf      General protection fault (#GP)");
    crate::println!("    ud       Invalid opcode (#UD)");
    crate::println!("  fault help   This help");
}

fn help_signal() {
    crate::println!("signal — kernel signal / callback API");
    crate::println!("  signal              Show pending / delivered stats");
    crate::println!("  signal list         List built-in signal numbers");
    crate::println!("  signal send <n>     Schedule signal n (queue)");
    crate::println!("  signal raise <n>    Schedule + process immediately");
    crate::println!("  signal process      Deliver all pending now");
    crate::println!("  signal help         This help");
    crate::println!("  Alias: sig");
    crate::println!();
    crate::println!("Built-in numbers: WARNING=20 KEYBOARD=2 TERM=15 PAGEFAULT=14 GPF=13");
}

fn help_idt() {
    crate::println!("idt");
    crate::println!("  Show IDT base, how many gates are present, IRQ summary.");
    crate::println!("  The IDT is created, filled (exceptions + keyboard), and lidt-registered at boot.");
}

fn cmd_about() {
    crate::println!("KFS — Kernel From Scratch (i686)");
    crate::println!("  arch:     i686 freestanding (no_std)");
    crate::println!("  boot:     Multiboot 1 + GRUB");
    crate::println!("  gdt:      {} entries @ 0x{:08X}", entry_count(), GDT_ADDRESS);
    crate::println!(
        "  selectors kcode={:#x} kdata={:#x} kstack={:#x}",
        KERNEL_CODE_SELECTOR,
        KERNEL_DATA_SELECTOR,
        KERNEL_STACK_SELECTOR
    );
    crate::println!(
        "  selectors ucode={:#x} udata={:#x} ustack={:#x} tss={:#x}",
        USER_CODE_SELECTOR,
        USER_DATA_SELECTOR,
        USER_STACK_SELECTOR,
        crate::gdt::TSS_SELECTOR
    );
    crate::println!("  irq:      keyboard IRQ1 -> vector 33 | timer IRQ0");
    crate::println!("  syscall:  int 0x80 (DPL=3) — try `user`");
    crate::println!("  screens:  6 virtual VGA consoles");
    if crate::memory::is_initialized() {
        crate::println!(
            "  pmm:      {} frames free / {} total ({} KiB free)",
            crate::memory::free_frames(),
            crate::memory::total_frames(),
            crate::memory::free_frames() * crate::memory::FRAME_SIZE / 1024
        );
    } else {
        crate::println!("  pmm:      not initialized");
    }
    if crate::memory::paging_enabled() {
        crate::println!(
            "  paging:   ON  CR3={:#010x}",
            crate::memory::page_directory_phys()
                .map(|p| p.as_u32())
                .unwrap_or(0)
        );
        crate::println!(
            "  spaces:   user < {:#x} <= kernel",
            crate::memory::KERNEL_SPACE_START
        );
    } else {
        crate::println!("  paging:   OFF");
    }
    if crate::memory::heap_initialized() {
        crate::println!(
            "  heap:     [{:#x} .. {:#x}) used={} B live={}",
            crate::memory::heap_start(),
            crate::memory::heap_end(),
            crate::memory::heap_used_bytes(),
            crate::memory::heap_alloc_count()
        );
    } else {
        crate::println!("  heap:     not initialized");
    }
}

fn cmd_clear() {
    out::clear();
}

fn cmd_echo(rest: &str) {
    if rest.is_empty() {
        crate::println!();
    } else {
        crate::println!("{}", rest);
    }
}

fn cmd_gdt() {
    crate::println!("GDT @ 0x{:08X}  ({} entries)", GDT_ADDRESS, entry_count());
    crate::println!("idx  name           sel    base       limit      access gran  flags");
    crate::println!("---- -------------- ------ ---------- ---------- ------ ----  -----");

    for i in 0..entry_count() {
        let Some(e) = read_installed_entry(i) else {
            continue;
        };
        let name = ENTRY_NAMES.get(i).copied().unwrap_or("?");
        let sel = (i as u16) << 3;
        let base = e.base();
        let limit = e.limit();
        let g = (e.granularity >> 7) & 1;

        crate::print!(
            "{:>3}  {:<14} {:#06x} {:#010x} {:#010x} {:#04x}   {:#04x}  ",
            i,
            name,
            sel,
            base,
            limit,
            e.access,
            e.granularity
        );
        if e.access == 0 {
            crate::println!("(null)");
        } else {
            crate::println!(
                "{} {} {}{}",
                if e.access & 0x80 != 0 { "P" } else { "-" },
                dpl_str(e.access),
                type_str(e.access),
                if g != 0 { " G" } else { "" },
            );
        }
    }

    crate::println!();
    crate::println!(
        "note: user RPL=3 selectors → ucode={:#x} udata={:#x} ustack={:#x}",
        USER_CODE_SELECTOR,
        USER_DATA_SELECTOR,
        USER_STACK_SELECTOR
    );
}

fn dpl_str(access: u8) -> &'static str {
    match (access >> 5) & 0b11 {
        0 => "DPL0",
        1 => "DPL1",
        2 => "DPL2",
        _ => "DPL3",
    }
}

fn type_str(access: u8) -> &'static str {
    if access & 0x10 == 0 {
        // System segment (S=0): TSS, LDT, gates…
        return match access & 0x0F {
            0x9 => "TSS-32",
            0xB => "TSS-busy",
            0x2 => "LDT",
            _ => "system",
        };
    }
    if access & 0x08 != 0 {
        if access & 0x02 != 0 {
            "code XR"
        } else {
            "code X"
        }
    } else if access & 0x04 != 0 {
        if access & 0x02 != 0 {
            "stack EW"
        } else {
            "stack E"
        }
    } else if access & 0x02 != 0 {
        "data RW"
    } else {
        "data R"
    }
}

fn cmd_stack(rest: &str) {
    let first = rest.split_whitespace().next().unwrap_or("");
    if first == "help" || first == "?" {
        help_stack();
        return;
    }
    let n = parse_usize(if first.is_empty() { "16" } else { first }).unwrap_or(16).min(64);
    let esp: u32;
    let ebp: u32;
    unsafe {
        asm!("mov {}, esp", out(reg) esp, options(nomem, nostack, preserves_flags));
        asm!("mov {}, ebp", out(reg) ebp, options(nomem, nostack, preserves_flags));
    }

    crate::println!("stack dump: ESP={:#010x} EBP={:#010x} ({} dwords)", esp, ebp, n);
    crate::println!("  addr       value      ascii");

    for i in 0..n {
        let addr = esp.wrapping_add((i as u32) * 4);
        let val = unsafe { core::ptr::read_volatile(addr as *const u32) };
        crate::print!("  {:#010x} {:#010x}  ", addr, val);
        print_ascii_u32(val);
        crate::println!();
    }
}

fn print_ascii_u32(val: u32) {
    let bytes = val.to_le_bytes();
    crate::print!("|");
    for b in bytes {
        let c = if (0x20..0x7f).contains(&b) {
            b as char
        } else {
            '.'
        };
        crate::print!("{}", c);
    }
    crate::print!("|");
}

fn cmd_regs() {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    let esi: u32;
    let edi: u32;
    let esp: u32;
    let ebp: u32;
    let eflags: u32;
    let cs: u16;
    let ds: u16;
    let es: u16;
    let fs: u16;
    let gs: u16;
    let ss: u16;

    // Split into small asm blocks — one large block exhausts the i386 reg file.
    unsafe {
        asm!("mov {}, eax", out(reg) eax, options(nomem, nostack, preserves_flags));
        asm!("mov {}, ebx", out(reg) ebx, options(nomem, nostack, preserves_flags));
        asm!("mov {}, ecx", out(reg) ecx, options(nomem, nostack, preserves_flags));
        asm!("mov {}, edx", out(reg) edx, options(nomem, nostack, preserves_flags));
        asm!("mov {}, esi", out(reg) esi, options(nomem, nostack, preserves_flags));
        asm!("mov {}, edi", out(reg) edi, options(nomem, nostack, preserves_flags));
        asm!("mov {}, esp", out(reg) esp, options(nomem, nostack, preserves_flags));
        asm!("mov {}, ebp", out(reg) ebp, options(nomem, nostack, preserves_flags));
        asm!("pushfd; pop {}", out(reg) eflags, options(nostack));
        asm!("mov ax, cs", out("ax") cs, options(nomem, nostack, preserves_flags));
        asm!("mov ax, ds", out("ax") ds, options(nomem, nostack, preserves_flags));
        asm!("mov ax, es", out("ax") es, options(nomem, nostack, preserves_flags));
        asm!("mov ax, fs", out("ax") fs, options(nomem, nostack, preserves_flags));
        asm!("mov ax, gs", out("ax") gs, options(nomem, nostack, preserves_flags));
        asm!("mov ax, ss", out("ax") ss, options(nomem, nostack, preserves_flags));
    }

    crate::println!("registers:");
    crate::println!(
        "  EAX={:#010x}  EBX={:#010x}  ECX={:#010x}  EDX={:#010x}",
        eax, ebx, ecx, edx
    );
    crate::println!(
        "  ESI={:#010x}  EDI={:#010x}  EBP={:#010x}  ESP={:#010x}",
        esi, edi, ebp, esp
    );
    crate::println!(
        "  EFLAGS={:#010x}  IF={}",
        eflags,
        if eflags & (1 << 9) != 0 { 1 } else { 0 }
    );
    crate::println!(
        "  CS={:#06x} DS={:#06x} ES={:#06x} FS={:#06x} GS={:#06x} SS={:#06x}",
        cs, ds, es, fs, gs, ss
    );
    crate::println!(
        "  expected: CS={:#x} DS={:#x} SS={:#x}",
        KERNEL_CODE_SELECTOR,
        KERNEL_DATA_SELECTOR,
        KERNEL_STACK_SELECTOR
    );
}

fn cmd_idt() {
    use crate::interrupts::{idt_base, idt_is_registered, present_gate_count};

    crate::println!("IDT (Interrupt Descriptor Table):");
    crate::println!("  registered: {}", if idt_is_registered() { "yes (lidt)" } else { "no" });
    crate::println!("  base:       {:#010x}", idt_base());
    crate::println!("  size:       256 slots");
    crate::println!("  present:    {} gates filled", present_gate_count());
    crate::println!("  exceptions: vectors 0-31 → exception stubs → panic");
    crate::println!(
        "  keyboard:   IRQ1 -> vector 33, CS={:#x}",
        KERNEL_CODE_SELECTOR
    );
    crate::println!("  PIC:        master 0x20, slave 0x28; IRQ1 unmasked");
    crate::println!("  test:       `fault div` / `panic`");
}

fn cmd_signal(rest: &str) {
    use crate::interrupts::{
        delivered_count, has_handler, pending_count, process_signals, raise_signal,
        schedule_signal, sig, MAX_SIGNALS,
    };

    let mut parts = rest.split_whitespace();
    let sub = parts.next().unwrap_or("");

    if sub == "help" || sub == "?" {
        help_signal();
        return;
    }

    match sub {
        "" | "info" | "stat" => {
            crate::println!("Signal system:");
            crate::println!("  pending:   {}", pending_count());
            crate::println!("  delivered: {}", delivered_count());
            crate::println!("  max sigs:  {}", MAX_SIGNALS);
            crate::println!("  handlers:  WARNING={} TERM={}", has_handler(sig::WARNING), has_handler(sig::TERM));
        }
        "list" => {
            crate::println!("Built-in signals:");
            crate::println!("  {}  ALARM", sig::ALARM);
            crate::println!("  {}  KEYBOARD", sig::KEYBOARD);
            crate::println!("  {}  GPF", sig::GPF);
            crate::println!("  {}  PAGEFAULT", sig::PAGEFAULT);
            crate::println!("  {}  TERM", sig::TERM);
            crate::println!("  {}  WARNING", sig::WARNING);
            crate::println!("  {}+ USER0..", sig::USER0);
        }
        "send" | "schedule" => {
            let Some(n) = parts.next() else {
                crate::println!("usage: signal send <number>");
                return;
            };
            let Some(sig_n) = parse_u32(n) else {
                crate::println!("bad signal number");
                return;
            };
            if schedule_signal(sig_n) {
                crate::println!("scheduled signal {} (pending={})", sig_n, pending_count());
            } else {
                crate::println!("failed to schedule {}", sig_n);
            }
        }
        "raise" => {
            let Some(n) = parts.next() else {
                crate::println!("usage: signal raise <number>");
                return;
            };
            let Some(sig_n) = parse_u32(n) else {
                crate::println!("bad signal number");
                return;
            };
            if sig_n == sig::TERM {
                crate::println!("raising TERM (will halt)...");
            }
            raise_signal(sig_n);
            crate::println!("raised {}; delivered total {}", sig_n, delivered_count());
        }
        "process" | "flush" => {
            let before = pending_count();
            process_signals();
            crate::println!("processed (had {} pending); delivered total {}", before, delivered_count());
        }
        _ => crate::println!("unknown signal subcommand (try `signal help`)"),
    }
}

fn cmd_mem(rest: &str) {
    let mut it = rest.split_whitespace();
    let addr_s = it.next();
    let n_s = it.next();
    if matches!(addr_s, Some("help") | Some("?")) || addr_s.is_none() {
        help_mem();
        return;
    }
    let addr_s = addr_s.unwrap();
    let Some(addr) = parse_u32(addr_s) else {
        crate::println!("bad address: {}", addr_s);
        return;
    };
    let n = parse_usize(n_s.unwrap_or("16")).unwrap_or(16).min(256);

    crate::println!("memory @ {:#010x} ({} bytes):", addr, n);
    let mut i = 0;
    while i < n {
        crate::print!("  {:#010x}: ", addr.wrapping_add(i as u32));
        let row = (n - i).min(16);
        for j in 0..row {
            let b = unsafe { core::ptr::read_volatile((addr as *const u8).wrapping_add(i + j)) };
            crate::print!("{:02x} ", b);
        }
        for _ in row..16 {
            crate::print!("   ");
        }
        crate::print!(" |");
        for j in 0..row {
            let b = unsafe { core::ptr::read_volatile((addr as *const u8).wrapping_add(i + j)) };
            let c = if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            };
            crate::print!("{}", c);
        }
        crate::println!("|");
        i += row;
    }
}

fn cmd_color(rest: &str) {
    let name = rest.split_whitespace().next();
    if matches!(name, None | Some("help") | Some("?")) {
        help_color();
        return;
    }
    let name = name.unwrap();
    let fg = match name {
        "white" => Color::White,
        "green" => Color::LightGreen,
        "cyan" => Color::LightCyan,
        "yellow" => Color::Yellow,
        "red" => Color::LightRed,
        "magenta" | "pink" => Color::Pink,
        "blue" => Color::LightBlue,
        "gray" | "grey" => Color::LightGray,
        "brown" => Color::Brown,
        "dark" => Color::DarkGray,
        _ => {
            crate::println!("unknown color `{}`", name);
            return;
        }
    };
    out::change_color(ColorCode::new(fg, Color::Black));
    crate::println!("color set to {}", name);
}

fn cmd_reboot() -> ! {
    crate::println!("rebooting...");
    unsafe {
        for _ in 0..100_000 {
            if kbc_status() & 0x02 == 0 {
                break;
            }
        }
        outb(0x64, 0xFE);
        outw(0x604, 0x2000);
    }
    loop {
        unsafe {
            asm!("cli; hlt");
        }
    }
}

fn kbc_status() -> u8 {
    unsafe {
        let mut v: u8;
        asm!("in al, dx", in("dx") 0x64u16, out("al") v, options(nostack, preserves_flags));
        v
    }
}

fn cmd_halt() -> ! {
    crate::println!("halting.");
    loop {
        unsafe {
            asm!("cli; hlt");
        }
    }
}


fn cmd_heap(rest: &str) {
    use crate::memory::{
        heap_alloc_count, heap_end, heap_initialized, heap_start, heap_used_bytes, kfree, kmalloc,
        ksize, virt_alloc, virt_free, virt_size, KERNEL_HEAP_MAX,
    };

    let mut parts = rest.split_whitespace();
    let sub = parts.next().unwrap_or("");

    if sub == "help" || sub == "?" {
        help_heap();
        return;
    }

    if !heap_initialized() {
        crate::println!("heap not initialized");
        return;
    }

    match sub {
        "" | "info" | "stat" => {
            crate::println!("Kernel heap (virtual):");
            crate::println!(
                "  range:   [{:#010x}, {:#010x})",
                heap_start(),
                heap_start() + KERNEL_HEAP_MAX
            );
            crate::println!("  mapped:   [{:#010x}, {:#010x})", heap_start(), heap_end());
            crate::println!("  used:     {} bytes (approx payload)", heap_used_bytes());
            crate::println!("  live:     {} allocations", heap_alloc_count());
        }
        "alloc" => {
            let n = parse_usize(parts.next().unwrap_or("64")).unwrap_or(64);
            match kmalloc(n) {
                Some(p) => {
                    let sz = ksize(p).unwrap_or(0);
                    crate::println!("kmalloc({}) -> {:#010x} (ksize={})", n, p as u32, sz);
                }
                None => crate::println!("kmalloc({}) failed (OOM)", n),
            }
        }
        "free" => {
            let Some(a) = parts.next() else {
                crate::println!("usage: heap free <hex_ptr>");
                return;
            };
            let Some(addr) = parse_u32(a) else {
                crate::println!("bad pointer");
                return;
            };
            let p = addr as *mut u8;
            if let Some(sz) = ksize(p) {
                kfree(p);
                crate::println!("kfree({:#010x}) size was {}", addr, sz);
            } else {
                crate::println!("kfree({:#010x}): not a live heap block", addr);
            }
        }
        "size" => {
            let Some(a) = parts.next() else {
                crate::println!("usage: heap size <hex_ptr>");
                return;
            };
            let Some(addr) = parse_u32(a) else {
                crate::println!("bad pointer");
                return;
            };
            match ksize(addr as *mut u8) {
                Some(sz) => crate::println!("ksize({:#010x}) = {}", addr, sz),
                None => crate::println!("ksize({:#010x}) = unknown", addr),
            }
        }
        "test" => {
            let before = heap_alloc_count();
            let a = virt_alloc(32).expect("alloc 32");
            let b = virt_alloc(200).expect("alloc 200");
            let sa = virt_size(a).unwrap_or(0);
            let sb = virt_size(b).unwrap_or(0);
            crate::println!("  A {:#010x} ksize {}", a as u32, sa);
            crate::println!("  B {:#010x} ksize {}", b as u32, sb);
            // write pattern
            unsafe {
                a.write_volatile(0xA5);
                b.write_volatile(0x5A);
            }
            let ok_rw = unsafe { a.read_volatile() == 0xA5 && b.read_volatile() == 0x5A };
            virt_free(a);
            virt_free(b);
            let after = heap_alloc_count();
            if ok_rw && sa >= 32 && sb >= 200 && after == before {
                crate::println!("  OK");
            } else {
                crate::println!(
                    "  FAIL ok_rw={} sa={} sb={} before={} after={}",
                    ok_rw, sa, sb, before, after
                );
            }
        }
        _ => {
            crate::println!("unknown heap subcommand (try `heap help`)");
        }
    }
}

fn cmd_vmm(rest: &str) {
    use crate::memory::{
        create_page, get_page, map_page, page_directory_phys, paging_enabled, unmap_page,
        virt_to_phys, PhysAddr, FRAME_SIZE, KERNEL_SPACE_START, PAGE_KERNEL_RW, PAGE_USER_RW,
        USER_SPACE_END, USER_SPACE_START,
    };
    use crate::memory::paging::{debug_cr0, debug_cr3};

    let mut parts = rest.split_whitespace();
    let sub = parts.next().unwrap_or("");

    if sub == "help" || sub == "?" {
        help_vmm();
        return;
    }

    if !paging_enabled() {
        crate::println!("paging is not enabled");
        return;
    }

    match sub {
        "" | "info" | "stat" => {
            crate::println!("Virtual memory / paging:");
            crate::println!("  enabled:  yes");
            crate::println!("  CR0:      {:#010x} (PG={})", debug_cr0(), (debug_cr0() >> 31) & 1);
            crate::println!("  CR3:      {:#010x}", debug_cr3());
            crate::println!(
                "  PD phys:  {:#010x}",
                page_directory_phys().map(|p| p.as_u32()).unwrap_or(0)
            );
            crate::println!(
                "  user VA:  [{:#010x}, {:#010x})",
                USER_SPACE_START,
                USER_SPACE_END
            );
            crate::println!("  kernel VA: >= {:#010x}", KERNEL_SPACE_START);
            crate::println!("  note: kernel image is identity-mapped low with SUPERVISOR pages");
        }
        "page" | "get" | "walk" => {
            let Some(a) = parts.next() else {
                crate::println!("usage: vmm page <virt_hex>");
                return;
            };
            let Some(virt) = parse_u32(a) else {
                crate::println!("bad address");
                return;
            };
            match get_page(virt) {
                None => crate::println!("no page directory"),
                Some(info) => {
                    crate::println!("page for virt {:#010x}:", virt);
                    crate::println!("  page VA:  {:#010x}", info.virt);
                    crate::println!(
                        "  present:  {}  writable: {}  user: {}",
                        info.present,
                        info.writable,
                        info.user
                    );
                    if info.present {
                        crate::println!("  frame:    {:#010x}", info.phys);
                        crate::println!("  flags:    {:#05x}", info.flags);
                        if let Some(p) = virt_to_phys(virt) {
                            crate::println!("  translate({:#010x}) = {:#010x}", virt, p);
                        }
                    }
                }
            }
        }
        "map" => {
            let Some(v) = parts.next() else {
                crate::println!("usage: vmm map <virt_hex> [phys_hex]");
                return;
            };
            let Some(virt) = parse_u32(v) else {
                crate::println!("bad virt");
                return;
            };
            let virt = virt & !0xFFF;
            if let Some(p) = parts.next() {
                let Some(phys) = parse_u32(p) else {
                    crate::println!("bad phys");
                    return;
                };
                map_page(virt, PhysAddr::new(phys & !0xFFF), PAGE_KERNEL_RW);
                crate::println!("mapped {:#010x} -> {:#010x} (kernel RW)", virt, phys & !0xFFF);
            } else {
                let frame = create_page(virt, PAGE_KERNEL_RW);
                crate::println!(
                    "create_page {:#010x} -> frame {:#010x}",
                    virt,
                    frame.as_u32()
                );
            }
        }
        "unmap" => {
            let Some(v) = parts.next() else {
                crate::println!("usage: vmm unmap <virt_hex>");
                return;
            };
            let Some(virt) = parse_u32(v) else {
                crate::println!("bad virt");
                return;
            };
            let virt = virt & !0xFFF;
            unmap_page(virt);
            crate::println!("unmapped {:#010x}", virt);
        }
        "test" => {
            // Pick a page inside the identity window that is normally free of kernel image.
            let virt = 0x0200_0000u32;
            crate::println!("vmm test: map {:#x}", virt);
            if get_page(virt).map(|i| i.present).unwrap_or(false) {
                crate::println!("  page already present, skip");
                return;
            }
            let frame = create_page(virt, PAGE_KERNEL_RW);
            // write a marker through the virtual address
            unsafe {
                let p = virt as *mut u32;
                p.write_volatile(0xC0FFEE);
            }
            let read_back = unsafe { (virt as *const u32).read_volatile() };
            let phys = virt_to_phys(virt).unwrap_or(0);
            crate::println!(
                "  frame={:#x} phys_trans={:#x} read={:#x}",
                frame.as_u32(),
                phys,
                read_back
            );
            unmap_page(virt);
            // free the frame back to PMM
            crate::memory::free_frame(frame);
            let present_after = get_page(virt).map(|i| i.present).unwrap_or(false);
            if read_back == 0xC0FFEE && !present_after {
                crate::println!("  OK");
            } else {
                crate::println!("  FAIL");
            }
            let _ = (PAGE_USER_RW, FRAME_SIZE);
        }
        _ => {
            crate::println!("unknown vmm subcommand (try `vmm help`)");
        }
    }
}

fn cmd_pmm(rest: &str) {
    use crate::memory::{
        alloc_frame, frame_size, free_frame, free_frames, phys_alloc, phys_free, phys_size,
        total_frames, used_frames, PhysAddr, FRAME_SIZE,
    };

    let mut parts = rest.split_whitespace();
    let sub = parts.next().unwrap_or("");

    if sub == "help" || sub == "?" {
        help_pmm();
        return;
    }

    if !crate::memory::is_initialized() {
        crate::println!("PMM not initialized");
        return;
    }

    match sub {
        "" | "info" | "stat" | "stats" => {
            crate::println!("Physical Memory Manager:");
            crate::println!("  frame size:  {} bytes", frame_size());
            crate::println!("  total:       {} frames ({} KiB)", total_frames(), total_frames() * FRAME_SIZE / 1024);
            crate::println!("  used:        {} frames", used_frames());
            crate::println!("  free:        {} frames ({} KiB)", free_frames(), free_frames() * FRAME_SIZE / 1024);
        }
        "alloc" => {
            let n = parse_usize(parts.next().unwrap_or("4096")).unwrap_or(4096);
            match phys_alloc(n) {
                Some(addr) => {
                    let sz = phys_size(addr).unwrap_or(0);
                    crate::println!("phys_alloc({}) -> {:#010x} (size {} bytes)", n, addr.as_u32(), sz);
                }
                None => crate::println!("phys_alloc({}) failed (OOM)", n),
            }
        }
        "frame" => {
            match alloc_frame() {
                Some(addr) => crate::println!("alloc_frame() -> {:#010x}", addr.as_u32()),
                None => crate::println!("alloc_frame() failed (OOM)"),
            }
        }
        "free" => {
            let Some(a) = parts.next() else {
                crate::println!("usage: pmm free <hex_addr>");
                return;
            };
            let Some(addr) = parse_u32(a) else {
                crate::println!("bad address");
                return;
            };
            let pa = PhysAddr::new(addr);
            if let Some(sz) = phys_size(pa) {
                phys_free(pa);
                crate::println!("phys_free({:#010x}) size was {}", addr, sz);
            } else {
                free_frame(pa);
                crate::println!("free_frame({:#010x})", addr);
            }
        }
        "size" => {
            let Some(a) = parts.next() else {
                crate::println!("usage: pmm size <hex_addr>");
                return;
            };
            let Some(addr) = parse_u32(a) else {
                crate::println!("bad address");
                return;
            };
            match phys_size(PhysAddr::new(addr)) {
                Some(sz) => crate::println!("phys_size({:#010x}) = {} bytes", addr, sz),
                None => crate::println!("phys_size({:#010x}) = unknown", addr),
            }
        }
        "test" => {
            let before = free_frames();
            crate::println!("pmm test: free before = {}", before);
            let a = phys_alloc(100).expect("alloc 100");
            let b = phys_alloc(8192).expect("alloc 8192");
            let sa = phys_size(a).unwrap_or(0);
            let sb = phys_size(b).unwrap_or(0);
            crate::println!("  A {:#010x} size {}", a.as_u32(), sa);
            crate::println!("  B {:#010x} size {}", b.as_u32(), sb);
            if sa < 100 || sb < 8192 {
                crate::println!("  FAIL size too small");
                return;
            }
            phys_free(a);
            phys_free(b);
            let after = free_frames();
            crate::println!("  free after = {}", after);
            if after == before {
                crate::println!("  OK");
            } else {
                crate::println!("  FAIL free count mismatch");
            }
        }
        _ => {
            crate::println!("unknown pmm subcommand (try `pmm help`)");
        }
    }
}

fn cmd_panic(rest: &str) {
    let first = rest.split_whitespace().next().unwrap_or("");
    if first == "help" || first == "?" {
        help_panic();
        return;
    }
    // Diverging path — never returns
    if rest.is_empty() {
        crate::panic::kernel_panic("shell: intentional panic (type: panic <msg>)");
    } else {
        crate::panic::kernel_panic(rest);
    }
}

fn cmd_fault(rest: &str) {
    let kind = rest.split_whitespace().next().unwrap_or("").trim();
    if kind == "help" || kind == "?" {
        help_fault();
        return;
    }
    match kind {
        "div" | "divide" | "" => {
            crate::println!("triggering divide-by-zero (#DE)...");
            unsafe {
                // div by zero → exception vector 0
                core::arch::asm!(
                    "mov eax, 1",
                    "xor ecx, ecx",
                    "div ecx",
                    options(nostack)
                );
            }
        }
        "gpf" | "gp" => {
            crate::println!("triggering general protection fault (#GP)...");
            unsafe {
                // Load null selector into DS → #GP
                core::arch::asm!(
                    "mov ax, 0",
                    "mov ds, ax",
                    options(nostack)
                );
            }
        }
        "ud" | "opcode" => {
            crate::println!("triggering invalid opcode (#UD)...");
            unsafe {
                core::arch::asm!("ud2", options(nostack));
            }
        }
        _ => {
            crate::println!("usage: fault <div|gpf|ud>");
            crate::panic::kernel_panic("shell: bad fault kind");
        }
    }
    crate::panic::kernel_panic("shell: fault did not raise an exception");
}


fn help_ps() {
    crate::println!("ps");
    crate::println!("  List processes: pid, state, parent, uid, name");
}

fn help_fork() {
    crate::println!("fork  — create child process; prints child pid");
}

fn help_wait() {
    crate::println!("wait  — reap one zombie child (non-blocking)");
}

fn help_kill() {
    crate::println!("kill <pid> <sig>  — queue signal for process (next tick)");
}

fn help_getuid() {
    crate::println!("getuid  — print current uid");
}

fn help_socket() {
    crate::println!("socket create | connect <fd> <pid> | send <fd> <word> | recv <fd> | close <fd>");
}

fn help_ls() {
    crate::println!("ls [path]");
    crate::println!("  List directory (default: current working directory).");
}

fn help_cat() {
    crate::println!("cat <file> [file...]");
    crate::println!("  Print file contents (like Unix cat).");
}

fn help_pwd() {
    crate::println!("pwd");
    crate::println!("  Print this process's working directory.");
}

fn help_cd() {
    crate::println!("cd [path]");
    crate::println!("  Change this process's working directory (default: /).");
}

fn help_mkdir() {
    crate::println!("mkdir <path>");
    crate::println!("  Create a directory (ext2).");
}

fn help_touch() {
    crate::println!("touch <path>");
    crate::println!("  Create an empty file, or update timestamps if it exists.");
}

fn help_rm() {
    crate::println!("rm <file>");
    crate::println!("  Remove a regular file (not directories).");
}

fn help_rmdir() {
    crate::println!("rmdir <dir>");
    crate::println!("  Remove an empty directory.");
}

fn cmd_mkdir(rest: &str) {
    let path = rest.split_whitespace().next().unwrap_or("");
    if path.is_empty() || path == "help" {
        help_mkdir();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("mkdir: filesystem not mounted");
        return;
    }
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2_write::mkdir(cwd, path) {
        Ok(ino) => crate::println!("mkdir: created inode {}", ino),
        Err(e) => crate::println!("mkdir: {}", e),
    }
}

fn cmd_touch(rest: &str) {
    let path = rest.split_whitespace().next().unwrap_or("");
    if path.is_empty() || path == "help" {
        help_touch();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("touch: filesystem not mounted");
        return;
    }
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2_write::touch(cwd, path) {
        Ok(ino) => crate::println!("touch: inode {}", ino),
        Err(e) => crate::println!("touch: {}", e),
    }
}

fn cmd_rm(rest: &str) {
    let path = rest.split_whitespace().next().unwrap_or("");
    if path.is_empty() || path == "help" {
        help_rm();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("rm: filesystem not mounted");
        return;
    }
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2_write::unlink(cwd, path) {
        Ok(()) => crate::println!("rm: removed {}", path),
        Err(e) => crate::println!("rm: {}", e),
    }
}

fn cmd_rmdir(rest: &str) {
    let path = rest.split_whitespace().next().unwrap_or("");
    if path.is_empty() || path == "help" {
        help_rmdir();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("rmdir: filesystem not mounted");
        return;
    }
    let cwd = crate::fs::path::cwd_inode();
    match crate::fs::ext2_write::rmdir(cwd, path) {
        Ok(()) => crate::println!("rmdir: removed {}", path),
        Err(e) => crate::println!("rmdir: {}", e),
    }
}

fn cmd_pwd(rest: &str) {
    if rest.split_whitespace().next() == Some("help") {
        help_pwd();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("pwd: filesystem not mounted");
        return;
    }
    let mut buf = [0u8; 256];
    let n = crate::fs::path::getcwd_pretty(&mut buf);
    let s = core::str::from_utf8(&buf[..n]).unwrap_or("?");
    crate::println!("{}", s);
}

fn cmd_cd(rest: &str) {
    let path = rest.split_whitespace().next().unwrap_or("/");
    if path == "help" || path == "?" {
        help_cd();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("cd: filesystem not mounted");
        return;
    }
    match crate::fs::path::chdir(path) {
        Ok(()) => {}
        Err(e) => crate::println!("cd: {}", e),
    }
}

fn cmd_ls(rest: &str) {
    let path = rest.split_whitespace().next().unwrap_or(".");
    if path == "help" || path == "?" {
        help_ls();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("ls: filesystem not mounted");
        return;
    }
    let cwd = crate::fs::path::cwd_inode();
    let ino = match crate::fs::ext2::resolve_path(cwd, path) {
        Ok(i) => i,
        Err(e) => {
            crate::println!("ls: {}", e);
            return;
        }
    };
    match crate::fs::ext2::list_dir(ino) {
        Ok(n) => {
            for i in 0..n {
                if let Some(node) = crate::fs::vfs::cache_get(i) {
                    crate::println!(
                        "{:<4} {:<6} {:>8}  {}",
                        node.inode,
                        node.kind.as_str(),
                        node.size,
                        node.name_str()
                    );
                }
            }
        }
        Err(e) => crate::println!("ls: {}", e),
    }
}

fn cmd_cat(rest: &str) {
    if rest.is_empty() || rest.split_whitespace().next() == Some("help") {
        help_cat();
        return;
    }
    if !crate::fs::is_ready() {
        crate::println!("cat: filesystem not mounted");
        return;
    }
    let cwd = crate::fs::path::cwd_inode();
    for name in rest.split_whitespace() {
        let ino = match crate::fs::ext2::resolve_path(cwd, name) {
            Ok(i) => i,
            Err(e) => {
                crate::println!("cat: {}: {}", name, e);
                continue;
            }
        };
        if crate::fs::ext2::inode_is_dir(ino) {
            crate::println!("cat: {}: Is a directory", name);
            continue;
        }
        let mut buf = [0u8; 512];
        let mut off = 0u32;
        loop {
            match crate::fs::ext2::read_file(ino, off, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    for &b in &buf[..n] {
                        if b == 0 {
                            break;
                        }
                        if b == b'\n' {
                            crate::println!();
                        } else if (32..127).contains(&b) || b == b'\t' {
                            crate::print!("{}", b as char);
                        } else {
                            crate::print!(".");
                        }
                    }
                    off += n as u32;
                    if n < buf.len() {
                        break;
                    }
                }
                Err(e) => {
                    crate::println!("cat: {}: {}", name, e);
                    break;
                }
            }
        }
        // ensure newline at end of file if last char wasn't newline
        crate::println!();
    }
}


fn cmd_ps(rest: &str) {
    if rest.split_whitespace().next() == Some("help") {
        help_ps();
        return;
    }
    crate::println!("PID  STATE     PPID  UID  NAME");
    crate::process::for_each_process(|_i, p| {
        crate::println!(
            "{:<4} {:<9} {:<5} {:<4} {}",
            p.pid,
            p.state.as_str(),
            p.parent,
            p.uid,
            p.name_str()
        );
    });
    crate::println!("current pid={}", crate::process::current_pid());
}

fn cmd_fork(rest: &str) {
    if rest.split_whitespace().next() == Some("help") {
        help_fork();
        return;
    }
    let child = crate::process::fork();
    if child < 0 {
        crate::println!("fork failed");
    } else {
        crate::println!("fork ok — child pid={}", child);
    }
}

fn cmd_wait(rest: &str) {
    if rest.split_whitespace().next() == Some("help") {
        help_wait();
        return;
    }
    let mut status = 0i32;
    let pid = crate::process::wait(Some(&mut status));
    if pid < 0 {
        crate::println!("wait: no zombie child");
    } else {
        crate::println!("wait: reaped pid={} status={}", pid, status);
    }
}

fn cmd_kill(rest: &str) {
    let mut parts = rest.split_whitespace();
    let a = parts.next().unwrap_or("");
    if a == "help" || a.is_empty() {
        help_kill();
        return;
    }
    let Some(pid) = parse_u32(a).map(|x| x as i32) else {
        help_kill();
        return;
    };
    let sig = parse_u32(parts.next().unwrap_or("15")).unwrap_or(15);
    let r = crate::process::kill(pid, sig);
    crate::println!("kill({}, {}) -> {}", pid, sig, r);
}

fn cmd_getuid() {
    crate::println!("uid={}", crate::process::getuid());
}

fn cmd_socket(rest: &str) {
    use crate::process::{socket_close, socket_connect, socket_create, socket_recv, socket_send};
    let mut parts = rest.split_whitespace();
    let sub = parts.next().unwrap_or("help");
    match sub {
        "help" | "?" => help_socket(),
        "create" => crate::println!("socket fd={}", socket_create()),
        "connect" => {
            let fd = parse_u32(parts.next().unwrap_or("0")).unwrap_or(0) as i32;
            let peer = parse_u32(parts.next().unwrap_or("0")).unwrap_or(0) as i32;
            crate::println!("connect -> {}", socket_connect(fd, peer));
        }
        "send" => {
            let fd = parse_u32(parts.next().unwrap_or("0")).unwrap_or(0) as i32;
            let word = parts.next().unwrap_or("");
            let n = socket_send(fd, word.as_bytes());
            crate::println!("send -> {}", n);
        }
        "recv" => {
            let fd = parse_u32(parts.next().unwrap_or("0")).unwrap_or(0) as i32;
            let mut buf = [0u8; 64];
            let n = socket_recv(fd, &mut buf);
            if n > 0 {
                let s = core::str::from_utf8(&buf[..n as usize]).unwrap_or("?");
                crate::println!("recv ({}) `{}`", n, s);
            } else {
                crate::println!("recv -> {}", n);
            }
        }
        "close" => {
            let fd = parse_u32(parts.next().unwrap_or("0")).unwrap_or(0) as i32;
            crate::println!("close -> {}", socket_close(fd));
        }
        _ => help_socket(),
    }
}

fn parse_u32(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else if !s.is_empty()
        && s.chars().all(|c| c.is_ascii_hexdigit())
        && s.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'))
    {
        u32::from_str_radix(s, 16).ok()
    } else {
        s.parse::<u32>()
            .ok()
            .or_else(|| u32::from_str_radix(s, 16).ok())
    }
}

fn parse_usize(s: &str) -> Option<usize> {
    parse_u32(s).map(|v| v as usize)
}
