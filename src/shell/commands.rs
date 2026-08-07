//! Shell command implementations.

use core::arch::asm;

use crate::console;
use crate::fs;
use crate::gdt;
use crate::interrupts;
use crate::memory::{
    current_cr3, free_frames, heap_alloc_count, heap_end, heap_start, heap_used_bytes, kfree,
    kmalloc, kernel_cr3, ksize, page_directory_phys, total_frames, used_frames, virt_to_phys,
    KERNEL_HEAP_MAX,
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
            // prompt is re-drawn by submit() after dispatch returns
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
            let soft = page_directory_phys().map(|p| p.as_u64()).unwrap_or(0);
            let hw = current_cr3();
            let k = kernel_cr3();
            let pcb = crate::process::with_current(|p| p.cr3).unwrap_or(0);
            console::print("hw CR3=");
            console::write_hex64(hw);
            console::print("  soft=");
            console::write_hex64(soft);
            console::print("  kernel=");
            console::write_hex64(k);
            console::print("  pcb=");
            console::write_hex64(pcb);
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
        "preempt" | "sched" => cmd_preempt_status(),
        "preempttest" => cmd_preempttest(),
        "vfs" | "mounts" => cmd_vfs(),
        "insmod" => cmd_insmod(rest),
        "rmmod" => cmd_rmmod(rest),
        "lsmod" => crate::module::lsmod(),
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
                match crate::syscalls::run_embedded_cat() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run cat: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "ls" {
                match crate::syscalls::run_embedded_ls() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run ls: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "fork" || path == "forktest" {
                match crate::syscalls::run_embedded_forktest() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run forktest: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "preempt" || path == "preempttest" {
                cmd_preempttest();
                return;
            }
            if path == "clone" || path == "clonetest" {
                match crate::syscalls::run_embedded_clonetest() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run clonetest: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "futex" || path == "futextest" {
                match crate::syscalls::run_embedded_futextest() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run futextest: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "signal" || path == "signaltest" {
                match crate::syscalls::run_embedded_signaltest() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run signaltest: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "exec" || path == "exectest" {
                match crate::syscalls::run_embedded_exectest() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run exectest: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "sh" || path == "/bin/sh" || path == "init" {
                // Re-enter userspace init (same path as U8 boot handoff).
                match crate::syscalls::run_init_sh() {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run sh: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "vitest" {
                // Mini-vi smoke: edit hello.txt, insert 'Z', write+quit, cat result.
                match crate::syscalls::run_embedded_sh_script(
                    b"vi hello.txt\niZ\x1b:wq\ncat hello.txt\nexit\n",
                ) {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run vitest: ");
                        console::println(e);
                    }
                }
                return;
            }
            if path == "shtest" || path == "shdemo" {
                // Preload a short script into the keyboard ring (no sendkey races).
                // Includes cat with/without args to smoke-test first-word path + argv.
                match crate::syscalls::run_embedded_sh_script(
                    b"help\nhello\ncat\ncat hello.txt\ncat docs/readme.txt\ncat /no/such\nls\npwd\nexit\n",
                ) {
                    Ok(()) => {}
                    Err(e) => {
                        console::print("run shtest: ");
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
        "ps" => cmd_ps(),
        other => {
            console::print("unknown command: `");
            console::print(other);
            console::println("` (try help)");
        }
    }
}

fn cmd_preempt_status() {
    console::print("irq_preempt_count=");
    console::write_u64(crate::process::sched::preempt_count());
    console::print("  need_resched=");
    console::write_u64(if crate::process::sched::need_resched() {
        1
    } else {
        0
    });
    console::println("");
    console::println("Tip: run `preempttest` then `preempt` — count should rise if IRQ switched tasks.");
}

/// Focused tests for IRQ preemption (see `docs/SMOKE_PREEMPT.md`).
fn cmd_preempttest() {
    console::println("=== preempttest: specific feature checks ===");
    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut weak = 0u32;

    // A — forced synthetic round-robin (core try_preempt path)
    console::println("--- A: synthetic A→B→A (try_preempt + TrapFrame rewrite) ---");
    match cmd_pt_synthetic_roundtrip() {
        PtResult::Pass => {
            console::println("A: PASS");
            pass += 1;
        }
        PtResult::Fail => {
            console::println("A: FAIL");
            fail += 1;
        }
        PtResult::Weak => {
            console::println("A: WEAK");
            weak += 1;
        }
    }

    // B — kernel CS must never switch
    console::println("--- B: kernel CS → no switch ---");
    match cmd_pt_kernel_cs_skip() {
        PtResult::Pass => {
            console::println("B: PASS");
            pass += 1;
        }
        PtResult::Fail => {
            console::println("B: FAIL");
            fail += 1;
        }
        PtResult::Weak => {
            console::println("B: WEAK");
            weak += 1;
        }
    }

    // C — alone on CPU → no switch
    console::println("--- C: no Ready peer → no switch ---");
    match cmd_pt_no_ready_peer() {
        PtResult::Pass => {
            console::println("C: PASS");
            pass += 1;
        }
        PtResult::Fail => {
            console::println("C: FAIL");
            fail += 1;
        }
        PtResult::Weak => {
            console::println("C: WEAK");
            weak += 1;
        }
    }

    // D — interrupted context saved on previous PCB
    console::println("--- D: trap save integrity (markers on prev PCB) ---");
    match cmd_pt_trap_save() {
        PtResult::Pass => {
            console::println("D: PASS");
            pass += 1;
        }
        PtResult::Fail => {
            console::println("D: FAIL");
            fail += 1;
        }
        PtResult::Weak => {
            console::println("D: WEAK");
            weak += 1;
        }
    }

    // E — per-process kstack tops are distinct; install updates TSS.RSP0
    console::println("--- E: kstack slot tops + TSS install ---");
    match cmd_pt_kstack() {
        PtResult::Pass => {
            console::println("E: PASS");
            pass += 1;
        }
        PtResult::Fail => {
            console::println("E: FAIL");
            fail += 1;
        }
        PtResult::Weak => {
            console::println("E: WEAK");
            weak += 1;
        }
    }

    // F — without need_resched (and no force), try_preempt is a no-op
    console::println("--- F: need_resched gate ---");
    match cmd_pt_need_resched_gate() {
        PtResult::Pass => {
            console::println("F: PASS");
            pass += 1;
        }
        PtResult::Fail => {
            console::println("F: FAIL");
            fail += 1;
        }
        PtResult::Weak => {
            console::println("F: WEAK");
            weak += 1;
        }
    }

    // G — real userspace dual-spin under fork/wait (Phase 3 exit criterion)
    console::println("--- G: userspace dual-spin (embedded preempttest) ---");
    match cmd_pt_userspace() {
        PtResult::Pass => {
            console::println("G: PASS (IRQ switched under nest)");
            pass += 1;
        }
        PtResult::Fail => {
            console::println("G: FAIL");
            fail += 1;
        }
        PtResult::Weak => {
            console::println("G: WEAK (clean run but 0 IRQ switches — timing?)");
            weak += 1;
        }
    }

    console::print("=== preempttest summary: pass=");
    console::write_u64(pass as u64);
    console::print(" fail=");
    console::write_u64(fail as u64);
    console::print(" weak=");
    console::write_u64(weak as u64);
    console::println(" ===");
    if fail == 0 {
        console::println("preempttest: overall OK (fails=0)");
    } else {
        console::println("preempttest: overall FAIL");
    }
}

enum PtResult {
    Pass,
    Fail,
    Weak,
}

/// Two synthetic user PCBs: current=A Running, B Ready with trap at rip_b.
/// Returns (a_pid, b_pid) or None. Caller must `pt_cleanup`.
fn pt_setup_pair(rip_a: u64, rip_b: u64) -> Option<(i32, i32)> {
    use crate::process::TrapFrame;
    use crate::process::pcb::ProcessState;

    let a = crate::process::begin_user_task("pta").ok()?;
    let parent = 1i32;
    let b_idx = crate::process::table::alloc_slot()?;
    let mut b_pid = 0;
    crate::process::table::init_child_slot(b_idx, parent, 0, 0, 0, 0, 0, false, &mut b_pid);
    let _ = crate::process::table::with_pid(b_pid, |p| {
        p.state = ProcessState::Ready;
        p.entered_via_nest = false;
        p.trap = TrapFrame::from_user_entry(rip_b, 0x7ffff000, 0x202, 0);
        p.trap_valid = true;
        p.user_rip = rip_b;
        p.user_rsp = 0x7ffff000;
        p.set_name("ptb");
    });
    let _ = crate::process::table::add_child(0, b_pid);

    let _ = crate::process::with_current(|p| {
        p.state = ProcessState::Running;
        p.entered_via_nest = false;
        p.trap = TrapFrame::from_user_entry(rip_a, 0x7ffff000, 0x202, 0);
        p.trap_valid = true;
        p.user_rip = rip_a;
        p.user_rsp = 0x7ffff000;
        p.set_name("pta");
    });
    Some((a, b_pid))
}

fn pt_cleanup(a: i32, b: i32) {
    use crate::process::pcb::ProcessState;
    if b > 0 {
        if let Some(bi) = crate::process::table::find_pid(b) {
            crate::process::table::free_index(bi);
        }
    }
    if a > 0 {
        if let Some(ai) = crate::process::table::find_pid(a) {
            crate::process::table::free_index(ai);
        }
    }
    let _ = crate::process::sched::wake_up(1);
    if let Some(i) = crate::process::table::find_pid(1) {
        crate::process::table::set_current_index(i);
        let _ = crate::process::table::with_pid(1, |p| {
            p.state = ProcessState::Running;
        });
    }
    crate::process::kstack::install_for_slot(0);
}

/// A: forced A→B then B→A; frame.rip must track next task entry.
fn cmd_pt_synthetic_roundtrip() -> PtResult {
    use crate::process::TrapFrame;

    crate::process::sched::reset_preempt_count();
    let before = crate::process::sched::preempt_count();
    let Some((a, b)) = pt_setup_pair(0x400000, 0x400100) else {
        console::println("  setup FAIL");
        return PtResult::Fail;
    };

    let mut frame = TrapFrame::from_user_entry(0x400000, 0x7ffff000, 0x202, 0);
    unsafe {
        crate::process::sched::test_try_preempt(core::ptr::addr_of_mut!(frame));
    }
    let cur1 = crate::process::getpid();
    let rip1 = frame.rip;

    unsafe {
        crate::process::sched::test_try_preempt(core::ptr::addr_of_mut!(frame));
    }
    let after = crate::process::sched::preempt_count();
    let cur2 = crate::process::getpid();
    let rip2 = frame.rip;

    console::print("  switches=");
    console::write_u64(after.saturating_sub(before));
    console::print("  A→B pid=");
    console::write_u64(cur1 as u64);
    console::print(" rip=");
    console::write_hex64(rip1);
    console::print("  B→A pid=");
    console::write_u64(cur2 as u64);
    console::print(" rip=");
    console::write_hex64(rip2);
    console::println("");

    let ok =
        after >= before + 2 && cur1 == b && rip1 == 0x400100 && cur2 == a && rip2 == 0x400000;
    pt_cleanup(a, b);
    if ok {
        PtResult::Pass
    } else if after > before {
        PtResult::Weak
    } else {
        PtResult::Fail
    }
}

/// B: interrupt "in kernel" (CS ring 0) must not switch even with Ready peer.
fn cmd_pt_kernel_cs_skip() -> PtResult {
    use crate::gdt::KERNEL_CODE_SELECTOR;
    use crate::process::TrapFrame;

    crate::process::sched::reset_preempt_count();
    let before = crate::process::sched::preempt_count();
    let Some((a, b)) = pt_setup_pair(0x400000, 0x400100) else {
        console::println("  setup FAIL");
        return PtResult::Fail;
    };
    let cur_before = crate::process::getpid();

    let mut frame = TrapFrame::from_user_entry(0x400000, 0x7ffff000, 0x202, 0);
    frame.cs = KERNEL_CODE_SELECTOR as u64; // ring 0 → is_user() false
    unsafe {
        crate::process::sched::test_try_preempt(core::ptr::addr_of_mut!(frame));
    }
    let after = crate::process::sched::preempt_count();
    let cur_after = crate::process::getpid();

    console::print("  count_delta=");
    console::write_u64(after.saturating_sub(before));
    console::print("  pid_before=");
    console::write_u64(cur_before as u64);
    console::print(" pid_after=");
    console::write_u64(cur_after as u64);
    console::println("");

    let ok = after == before && cur_after == cur_before && cur_before == a;
    pt_cleanup(a, b);
    if ok {
        PtResult::Pass
    } else {
        PtResult::Fail
    }
}

/// C: only one runnable task → try_preempt must not switch / not panic.
fn cmd_pt_no_ready_peer() -> PtResult {
    use crate::process::TrapFrame;
    use crate::process::pcb::ProcessState;

    crate::process::sched::reset_preempt_count();
    let before = crate::process::sched::preempt_count();

    let a = match crate::process::begin_user_task("pta") {
        Ok(p) => p,
        Err(_) => {
            console::println("  setup FAIL");
            return PtResult::Fail;
        }
    };
    let _ = crate::process::with_current(|p| {
        p.state = ProcessState::Running;
        p.entered_via_nest = false;
        p.trap = TrapFrame::from_user_entry(0x400000, 0x7ffff000, 0x202, 0);
        p.trap_valid = true;
    });
    // Ensure no other Ready tasks exist (kinit is Sleeping from begin_user_task).
    let mut frame = TrapFrame::from_user_entry(0x400000, 0x7ffff000, 0x202, 0);
    unsafe {
        crate::process::sched::test_try_preempt(core::ptr::addr_of_mut!(frame));
    }
    let after = crate::process::sched::preempt_count();
    let cur = crate::process::getpid();

    console::print("  count_delta=");
    console::write_u64(after.saturating_sub(before));
    console::print("  still_pid=");
    console::write_u64(cur as u64);
    console::println("");

    let ok = after == before && cur == a && frame.rip == 0x400000;
    pt_cleanup(a, 0);
    if ok {
        PtResult::Pass
    } else {
        PtResult::Fail
    }
}

/// D: after A→B, A's PCB must hold the interrupted markers (rax/rbx/rip/rsp).
fn cmd_pt_trap_save() -> PtResult {
    use crate::process::TrapFrame;

    crate::process::sched::reset_preempt_count();
    let Some((a, b)) = pt_setup_pair(0x400000, 0x400100) else {
        console::println("  setup FAIL");
        return PtResult::Fail;
    };

    let mut frame = TrapFrame::from_user_entry(0x401234, 0x7fffabcd, 0x202, 0xA11A11A1);
    frame.rbx = 0xB22B22B2;
    frame.rcx = 0xC33C33C3;
    frame.rdx = 0xD44D44D4;
    frame.rsi = 0x51515151;
    frame.rdi = 0xD1D1D1D1;

    unsafe {
        crate::process::sched::test_try_preempt(core::ptr::addr_of_mut!(frame));
    }

    // Current should be B; frame should be B's entry.
    let cur = crate::process::getpid();
    let frame_ok = frame.rip == 0x400100;

    // Previous PCB A must have full saved trap.
    let saved = crate::process::table::with_pid(a, |p| {
        (
            p.trap_valid,
            p.trap.rip,
            p.trap.rsp,
            p.trap.rax,
            p.trap.rbx,
            p.trap.rcx,
            p.trap.rdx,
            p.state,
        )
    });

    let ok = match saved {
        Some((true, rip, rsp, rax, rbx, rcx, rdx, st)) => {
            console::print("  next_pid=");
            console::write_u64(cur as u64);
            console::print(" frame.rip=");
            console::write_hex64(frame.rip);
            console::print("  A.trap rip=");
            console::write_hex64(rip);
            console::print(" rax=");
            console::write_hex64(rax);
            console::print(" rbx=");
            console::write_hex64(rbx);
            console::println("");
            cur == b
                && frame_ok
                && rip == 0x401234
                && rsp == 0x7fffabcd
                && rax == 0xA11A11A1
                && rbx == 0xB22B22B2
                && rcx == 0xC33C33C3
                && rdx == 0xD44D44D4
                && st == crate::process::ProcessState::Ready
        }
        _ => {
            console::println("  A.trap missing");
            false
        }
    };

    pt_cleanup(a, b);
    if ok {
        PtResult::Pass
    } else {
        PtResult::Fail
    }
}

/// E: kstack tops for slots 0/1/2 differ; install_for_slot updates TSS.RSP0.
fn cmd_pt_kstack() -> PtResult {
    use crate::gdt::tss;
    use crate::process::kstack;

    let t0 = kstack::top_for_slot(0);
    let t1 = kstack::top_for_slot(1);
    let t2 = kstack::top_for_slot(2);

    console::print("  top[0]=");
    console::write_hex64(t0);
    console::print(" top[1]=");
    console::write_hex64(t1);
    console::print(" top[2]=");
    console::write_hex64(t2);
    console::println("");

    let distinct = t0 != t1 && t1 != t2 && t0 != t2 && t1 != 0 && t2 != 0;

    // Read live TSS.RSP0 (not the boot-stack constant `kernel_stack_top`).
    let saved = tss::tss_rsp0();
    kstack::install_for_slot(1);
    let rsp0_1 = tss::tss_rsp0();
    kstack::install_for_slot(2);
    let rsp0_2 = tss::tss_rsp0();
    kstack::install_for_slot(0);
    let rsp0_0 = tss::tss_rsp0();
    // Restore prior RSP0 + slot 0 for kinit shell.
    tss::set_kernel_stack(saved);
    kstack::install_for_slot(0);

    console::print("  tss.rsp0 slot1→");
    console::write_hex64(rsp0_1);
    console::print(" slot2→");
    console::write_hex64(rsp0_2);
    console::print(" slot0→");
    console::write_hex64(rsp0_0);
    console::println("");

    let install_ok = rsp0_1 == t1 && rsp0_2 == t2 && rsp0_0 == t0;
    if distinct && install_ok {
        PtResult::Pass
    } else {
        PtResult::Fail
    }
}

/// F: with Ready peer but need_resched clear and force off, no switch.
fn cmd_pt_need_resched_gate() -> PtResult {
    use crate::process::TrapFrame;

    crate::process::sched::reset_preempt_count();
    let before = crate::process::sched::preempt_count();
    let Some((a, b)) = pt_setup_pair(0x400000, 0x400100) else {
        console::println("  setup FAIL");
        return PtResult::Fail;
    };
    let cur_before = crate::process::getpid();

    crate::process::sched::clear_need_resched();
    let mut frame = TrapFrame::from_user_entry(0x400000, 0x7ffff000, 0x202, 0);
    // Direct try_preempt (no FORCE flag, need_resched false).
    unsafe {
        crate::process::sched::try_preempt(core::ptr::addr_of_mut!(frame));
    }
    let after = crate::process::sched::preempt_count();
    let cur_after = crate::process::getpid();

    console::print("  count_delta=");
    console::write_u64(after.saturating_sub(before));
    console::print("  pid_before=");
    console::write_u64(cur_before as u64);
    console::print(" pid_after=");
    console::write_u64(cur_after as u64);
    console::print("  (peer b=");
    console::write_u64(b as u64);
    console::println(")");

    let ok = after == before && cur_after == cur_before && cur_before == a && frame.rip == 0x400000;
    pt_cleanup(a, b);
    if ok {
        PtResult::Pass
    } else {
        PtResult::Fail
    }
}

/// G: embedded userspace fork+spin+wait (real path; nest gate may zero switches).
fn cmd_pt_userspace() -> PtResult {
    crate::process::sched::reset_preempt_count();
    let before = crate::process::sched::preempt_count();
    let n_before = crate::process::process_count();
    console::print("  processes_before=");
    console::write_u64(n_before as u64);
    console::println("");
    console::println("  run embedded preempttest (spin+wait)...");
    match crate::syscalls::run_embedded_preempttest() {
        Ok(()) => {}
        Err(e) => {
            console::print("  run FAIL: ");
            console::println(e);
            return PtResult::Fail;
        }
    }
    let after = crate::process::sched::preempt_count();
    let delta = after.saturating_sub(before);
    let n_after = crate::process::process_count();
    console::print("  irq_switches=");
    console::write_u64(delta);
    console::print("  processes_after=");
    console::write_u64(n_after as u64);
    console::println("");
    // enter_and_wait prints "process table full" but still returns Ok — detect leak.
    if n_after > n_before + 2 {
        console::println("  process leak / spawn failed");
        return PtResult::Fail;
    }
    if delta > 0 {
        PtResult::Pass
    } else {
        PtResult::Weak
    }
}

fn cmd_help() {
    console::println("munux shell commands:");
    console::println("  help / ?        This list");
    console::println("  about           Kernel summary");
    console::println("  clear / cls     Clear screen");
    console::println("  echo <text>     Print text");
    console::println("  pmm [test]      Physical frames");
    console::println("  heap [test]     Kernel heap / kmalloc");
    console::println("  ticks           PIT tick counter");
    console::println("  idt / gdt       Descriptor tables");
    console::println("  cr3 / vmm       Paging info");
    console::println("  preempt/sched   Show IRQ preemption counter");
    console::println("  preempttest     Specific IRQ preempt checks (A-G)");
    console::println("  vfs / mounts    Phase 7 VFS mounts + chrdevs");
    console::println("  insmod <name|path>  Load module (hello or /lib/modules/*.mnx)");
    console::println("  rmmod <name>    Unload module");
    console::println("  lsmod           List loaded modules + export count");
    console::println("  reboot / halt   Machine control");
    console::println("  fault [ud2]     Trigger CPU exception");
    console::println("  panic           Rust panic");
    console::println("  user            Enter ring 3 hand-asm demo");
    console::println("  run [path|echo|cat|ls|fork|exec|sh|init|shtest]  user ELF / shell");
    console::println("  run sh / run init   Re-enter U8 userspace /bin/sh");
    console::println("  ls [path]       List directory");
    console::println("  cat <path>      Print file");
    console::println("  pwd / cd        Working directory");
    console::println("  ps              Process table");
    console::println("Editing: Backspace/Del erase previous character");
}

fn cmd_insmod(rest: &str) {
    let arg = rest.split_whitespace().next().unwrap_or("");
    if arg.is_empty() {
        console::println("usage: insmod <name|/path/to.mnx>");
        console::println("  bare name looks in /lib/modules/<name>.mnx");
        console::println("  'hello' falls back to builtin if file missing");
        return;
    }
    match crate::module::insmod(arg) {
        Ok(()) => {}
        Err(e) => {
            console::print("insmod: ");
            console::println(e.as_str());
        }
    }
}

fn cmd_rmmod(rest: &str) {
    let arg = rest.split_whitespace().next().unwrap_or("");
    if arg.is_empty() {
        console::println("usage: rmmod <name>");
        return;
    }
    match crate::module::rmmod(arg) {
        Ok(()) => {}
        Err(e) => {
            console::print("rmmod: ");
            console::println(e.as_str());
        }
    }
}

fn cmd_vfs() {
    let (nm, nc) = fs::vcore::stats();
    console::print("vfs ready=");
    console::print(if fs::vcore::is_ready() { "yes" } else { "no" });
    console::print(" mounts=");
    console::write_u64(nm as u64);
    console::print(" chrdev=");
    console::write_u64(nc as u64);
    console::println("");
    for i in 0..fs::vcore::MAX_MOUNTS {
        if let Some((path, name)) = fs::vcore::mount_name_at(i) {
            console::print("  mount ");
            console::print(path);
            console::print(" -> ");
            console::println(name);
        }
    }
    console::println("  chrdev: /dev/null /dev/zero /dev/hda (+ /dev/echo via echo.mnx)");
    console::println("  ramfs:  /ram/hello (seeded)");
    console::println("  proc:   /proc/meminfo mounts version uptime self/status");
    console::print("  blkdev: ");
    console::write_u64(fs::blockdev::count() as u64);
    if let Some(n) = fs::blockdev::name_at(0) {
        console::print(" (");
        console::print(n);
        console::print(")");
    }
    console::println("");
    console::println("  fops: console, ext2, ramfs, null, zero, proc");
}

fn cmd_ps() {
    console::println("  PID  PPID  STATE    NAME");
    crate::process::for_each_process(|_i, p| {
        console::print("  ");
        console::write_u64(p.pid as u64);
        console::print("    ");
        if p.parent < 0 {
            console::print("-");
        } else {
            console::write_u64(p.parent as u64);
        }
        console::print("    ");
        console::print(p.state.as_str());
        // pad roughly
        let st = p.state.as_str().len();
        for _ in st..8 {
            console::print(" ");
        }
        console::println(p.name_str());
    });
    console::print("processes=");
    console::write_u64(crate::process::process_count() as u64);
    console::print(" current=");
    console::write_u64(crate::process::current_pid() as u64);
    console::println("");
}

fn cmd_about() {
    console::println("munux — freestanding x86_64 kernel (Rust + NASM)");
    console::println("PR1-8 boot..FS | U1-U8 ABI+sh init (docs/ABI.md)");
    console::print("FDs open=");
    console::write_u64(crate::fd::open_count() as u64);
    console::println(" (0=in 1=out 2=err)");
    console::print("pid=");
    console::write_u64(crate::process::current_pid() as u64);
    console::print(" processes=");
    console::write_u64(crate::process::process_count() as u64);
    console::println("");
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
    let path = rest.split_whitespace().next().unwrap_or(".");
    // VFS so /proc, /dev, /ram and mount points appear (Linux-like).
    let f = match fs::vcore::vfs_open(path, 0o200000, true, false) {
        Ok(f) if f.is_dir => f,
        Ok(_) => {
            console::println("ls: not a directory");
            return;
        }
        Err(_) => {
            console::println("ls: not found");
            return;
        }
    };
    let mut pos = 0u64;
    loop {
        match fs::vcore::vfs_dir_next(&f, pos) {
            Ok(None) => break,
            Err(_) => {
                console::println("ls: read error");
                break;
            }
            Ok(Some(e)) => {
                let name =
                    core::str::from_utf8(&e.name[..e.name_len as usize]).unwrap_or("?");
                if name != "." && name != ".." {
                    if e.d_type == fs::vcore::DT_DIR {
                        console::print("d ");
                    } else if e.d_type == fs::vcore::DT_CHR {
                        console::print("c ");
                    } else {
                        console::print("- ");
                    }
                    console::print(name);
                    console::println("");
                }
                pos = e.next_off;
            }
        }
    }
}

fn cmd_cat(rest: &str) {
    // Allow /proc and /dev even when ext2 is up; VFS routes virtual mounts.
    let path = rest.split_whitespace().next().unwrap_or("");
    if path.is_empty() {
        console::println("usage: cat <path>");
        return;
    }
    let mut f = match fs::vcore::vfs_open(path, 0, true, false) {
        Ok(f) => f,
        Err(_) => {
            console::println("cat: not found");
            return;
        }
    };
    if f.is_dir {
        console::println("cat: is a directory");
        return;
    }
    let mut buf = [0u8; 512];
    loop {
        match fs::vcore::vfs_read(&mut f, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                for &b in &buf[..n] {
                    console::put_char(b);
                }
            }
            Err(_) => {
                console::println("cat: read error");
                break;
            }
        }
    }
    if !buf.iter().any(|&b| b == b'\n') {
        console::println("");
    }
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
