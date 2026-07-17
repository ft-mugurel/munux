//! Kernel panic path: save diagnostics, clean registers, print, stop.
//!
//! Subject requirements covered here:
//! - Interface to **save the stack** before a panic
//! - Interface to **clean registers** before a panic / halt

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// How many stack dwords we keep in the panic snapshot.
pub const PANIC_STACK_WORDS: usize = 32;

/// Saved general-purpose + segment registers at panic time.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct SavedRegisters {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
    pub ebp: u32,
    pub esp: u32,
    pub eflags: u32,
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub ss: u16,
    pub _pad: u16,
}

impl SavedRegisters {
    pub const fn zeroed() -> Self {
        Self {
            eax: 0,
            ebx: 0,
            ecx: 0,
            edx: 0,
            esi: 0,
            edi: 0,
            ebp: 0,
            esp: 0,
            eflags: 0,
            cs: 0,
            ds: 0,
            es: 0,
            fs: 0,
            gs: 0,
            ss: 0,
            _pad: 0,
        }
    }
}

/// Stack snapshot: addresses + values.
#[derive(Clone, Copy)]
pub struct SavedStack {
    pub base_esp: u32,
    pub words: [u32; PANIC_STACK_WORDS],
    pub count: usize,
}

impl SavedStack {
    pub const fn empty() -> Self {
        Self {
            base_esp: 0,
            words: [0; PANIC_STACK_WORDS],
            count: 0,
        }
    }
}

static IN_PANIC: AtomicBool = AtomicBool::new(false);
static mut SAVED_REGS: SavedRegisters = SavedRegisters::zeroed();
static mut SAVED_STACK: SavedStack = SavedStack::empty();
static STACK_SAVED: AtomicBool = AtomicBool::new(false);
static REGS_SAVED: AtomicBool = AtomicBool::new(false);
static DUMP_WORDS: AtomicUsize = AtomicUsize::new(PANIC_STACK_WORDS);

// ---------------------------------------------------------------------------
// Public interfaces (subject wording)
// ---------------------------------------------------------------------------

/// **Interface to save the stack before a panic.**
///
/// Copies up to `max_words` dwords starting at the current ESP into a global
/// buffer that survives until halt (readable via [`last_saved_stack`]).
pub fn save_stack_before_panic(max_words: usize) {
    let n = max_words.min(PANIC_STACK_WORDS).max(1);
    DUMP_WORDS.store(n, Ordering::Relaxed);

    let esp: u32;
    unsafe {
        asm!("mov {}, esp", out(reg) esp, options(nomem, nostack, preserves_flags));
    }

    let mut snap = SavedStack::empty();
    snap.base_esp = esp;
    snap.count = n;
    for i in 0..n {
        let addr = esp.wrapping_add((i as u32) * 4);
        // Best-effort: if address is nonsense, store 0.
        let val = unsafe {
            // Only touch identity-mapped low memory roughly; still try read.
            core::ptr::read_volatile(addr as *const u32)
        };
        snap.words[i] = val;
    }
    unsafe {
        SAVED_STACK = snap;
    }
    STACK_SAVED.store(true, Ordering::SeqCst);
}

/// **Interface to clean registers before a panic / halt.**
///
/// 1. Snapshots current GPRs/segments into [`last_saved_registers`].
/// 2. Zeroes general-purpose registers (except ESP — stack must stay valid).
///
/// Call this after [`save_stack_before_panic`] so the stack dump still sees
/// the real stack contents.
pub fn clean_registers_before_halt() {
    // 1) Save first
    let mut r = SavedRegisters::zeroed();
    unsafe {
        asm!("mov {}, eax", out(reg) r.eax, options(nomem, nostack, preserves_flags));
        asm!("mov {}, ebx", out(reg) r.ebx, options(nomem, nostack, preserves_flags));
        asm!("mov {}, ecx", out(reg) r.ecx, options(nomem, nostack, preserves_flags));
        asm!("mov {}, edx", out(reg) r.edx, options(nomem, nostack, preserves_flags));
        asm!("mov {}, esi", out(reg) r.esi, options(nomem, nostack, preserves_flags));
        asm!("mov {}, edi", out(reg) r.edi, options(nomem, nostack, preserves_flags));
        asm!("mov {}, ebp", out(reg) r.ebp, options(nomem, nostack, preserves_flags));
        asm!("mov {}, esp", out(reg) r.esp, options(nomem, nostack, preserves_flags));
        asm!("pushfd; pop {}", out(reg) r.eflags, options(nostack));
        asm!("mov ax, cs", out("ax") r.cs, options(nomem, nostack, preserves_flags));
        asm!("mov ax, ds", out("ax") r.ds, options(nomem, nostack, preserves_flags));
        asm!("mov ax, es", out("ax") r.es, options(nomem, nostack, preserves_flags));
        asm!("mov ax, fs", out("ax") r.fs, options(nomem, nostack, preserves_flags));
        asm!("mov ax, gs", out("ax") r.gs, options(nomem, nostack, preserves_flags));
        asm!("mov ax, ss", out("ax") r.ss, options(nomem, nostack, preserves_flags));
        SAVED_REGS = r;
    }
    REGS_SAVED.store(true, Ordering::SeqCst);

    // 2) Clean (zero) scratch GPRs — keep ESP/EBP/SS for a stable halt.
    unsafe {
        asm!(
            "xor eax, eax",
            "xor ebx, ebx",
            "xor ecx, ecx",
            "xor edx, edx",
            "xor esi, esi",
            "xor edi, edi",
            // leave ebp/esp alone
            options(nostack)
        );
    }
}

/// Accessors for shell / debugging after a panic path has run partially.
pub fn last_saved_registers() -> Option<SavedRegisters> {
    if REGS_SAVED.load(Ordering::SeqCst) {
        Some(unsafe { SAVED_REGS })
    } else {
        None
    }
}

pub fn last_saved_stack() -> Option<SavedStack> {
    if STACK_SAVED.load(Ordering::SeqCst) {
        Some(unsafe { SAVED_STACK })
    } else {
        None
    }
}

pub fn print_saved_diagnostics() {
    if let Some(r) = last_saved_registers() {
        crate::println!("--- saved registers ---");
        crate::println!(
            "EAX={:#010x} EBX={:#010x} ECX={:#010x} EDX={:#010x}",
            r.eax, r.ebx, r.ecx, r.edx
        );
        crate::println!(
            "ESI={:#010x} EDI={:#010x} EBP={:#010x} ESP={:#010x}",
            r.esi, r.edi, r.ebp, r.esp
        );
        crate::println!(
            "CS={:#06x} DS={:#06x} SS={:#06x} EFLAGS={:#010x}",
            r.cs, r.ds, r.ss, r.eflags
        );
    }
    if let Some(s) = last_saved_stack() {
        crate::println!("--- saved stack @ ESP={:#010x} ({} words) ---", s.base_esp, s.count);
        for i in 0..s.count {
            let addr = s.base_esp.wrapping_add((i as u32) * 4);
            crate::println!("  {:#010x}: {:#010x}", addr, s.words[i]);
        }
    }
}

// ---------------------------------------------------------------------------
// Halt / panic entry points
// ---------------------------------------------------------------------------

/// Halt forever (interrupts off). Prefer [`halt_clean`] from normal code.
pub fn halt() -> ! {
    loop {
        unsafe {
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

/// Clean registers, then halt (non-panic shutdown path).
pub fn halt_clean(reason: &str) -> ! {
    save_stack_before_panic(PANIC_STACK_WORDS);
    clean_registers_before_halt();
    crate::println!("halt: {}", reason);
    print_saved_diagnostics();
    halt();
}

/// Full panic: save stack → clean regs → print → halt.
pub fn kernel_panic(message: &str) -> ! {
    if IN_PANIC.swap(true, Ordering::SeqCst) {
        halt();
    }

    save_stack_before_panic(PANIC_STACK_WORDS);
    clean_registers_before_halt();

    crate::println!();
    crate::println!("********************************");
    crate::println!("***       KERNEL PANIC       ***");
    crate::println!("********************************");
    crate::println!("{}", message);
    print_saved_diagnostics();
    crate::println!("System halted.");
    crate::println!("********************************");

    halt();
}

/// Panic with reason + detail lines.
pub fn kernel_panic_with_detail(reason: &str, detail: &str) -> ! {
    if IN_PANIC.swap(true, Ordering::SeqCst) {
        halt();
    }

    save_stack_before_panic(PANIC_STACK_WORDS);
    clean_registers_before_halt();

    crate::println!();
    crate::println!("********************************");
    crate::println!("***       KERNEL PANIC       ***");
    crate::println!("********************************");
    crate::println!("{}", reason);
    crate::println!("{}", detail);
    print_saved_diagnostics();
    crate::println!("System halted.");
    crate::println!("********************************");

    halt();
}
