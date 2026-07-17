//! CPU exception handlers (vectors 0–31).
//!
//! On x86 these are faults/traps/aborts from the CPU itself (not PIC IRQs).
//! Without handlers, a fault triple-faults → black screen / reboot.
//! With handlers, we print a clear panic and halt.

use core::arch::asm;

use crate::interrupts::idt::register_interrupt_handler;
use crate::interrupts::signal::{self, sig};
use crate::panic::kernel_panic_with_detail;

/// Stack layout built by `multiboot/exceptions.asm` after `pusha`.
///
/// Memory grows down; `frame` points at the lowest address (EDI).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExceptionFrame {
    pub edi: u32,
    pub esi: u32,
    pub ebp: u32,
    pub esp_dummy: u32, // ESP pushed by pusha (not very useful)
    pub ebx: u32,
    pub edx: u32,
    pub ecx: u32,
    pub eax: u32,
    pub vector: u32,
    pub error_code: u32,
    pub eip: u32,
    pub cs: u32,
    pub eflags: u32,
}

/// Human-readable names for the first 32 CPU exceptions.
const EXCEPTION_NAMES: [&str; 32] = [
    "Divide-by-zero (#DE)",
    "Debug (#DB)",
    "Non-maskable interrupt (NMI)",
    "Breakpoint (#BP)",
    "Overflow (#OF)",
    "Bound range exceeded (#BR)",
    "Invalid opcode (#UD)",
    "Device not available (#NM)",
    "Double fault (#DF)",
    "Coprocessor segment overrun",
    "Invalid TSS (#TS)",
    "Segment not present (#NP)",
    "Stack-segment fault (#SS)",
    "General protection fault (#GP)",
    "Page fault (#PF)",
    "Reserved",
    "x87 floating-point (#MF)",
    "Alignment check (#AC)",
    "Machine check (#MC)",
    "SIMD floating-point (#XM)",
    "Virtualization (#VE)",
    "Control protection (#CP)",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Hypervisor injection",
    "VMM communication",
    "Security exception (#SX)",
    "Reserved",
];

/// Called from assembly for every CPU exception.
///
/// # Safety
/// `frame` must point at a valid [`ExceptionFrame`] on the interrupt stack.
#[no_mangle]
pub unsafe extern "C" fn exception_handler(frame: *const ExceptionFrame) -> ! {
    let f = &*frame;
    let vec = f.vector as usize;
    let name = if vec < EXCEPTION_NAMES.len() {
        EXCEPTION_NAMES[vec]
    } else {
        "Unknown exception"
    };

    // CR2 holds the faulting linear address for page faults (#PF = 14).
    let cr2 = if vec == 14 { read_cr2() } else { 0 };

    // Build a small detail string without heap allocation.
    let mut detail = DetailBuf::new();
    detail.push_str(name);
    detail.push_str("\n  vector=");
    detail.push_u32(f.vector);
    detail.push_str(" error=");
    detail.push_hex(f.error_code);
    detail.push_str("\n  EIP=");
    detail.push_hex(f.eip);
    detail.push_str(" CS=");
    detail.push_hex(f.cs);
    detail.push_str(" EFLAGS=");
    detail.push_hex(f.eflags);
    detail.push_str("\n  EAX=");
    detail.push_hex(f.eax);
    detail.push_str(" EBX=");
    detail.push_hex(f.ebx);
    detail.push_str(" ECX=");
    detail.push_hex(f.ecx);
    detail.push_str(" EDX=");
    detail.push_hex(f.edx);
    detail.push_str("\n  ESI=");
    detail.push_hex(f.esi);
    detail.push_str(" EDI=");
    detail.push_hex(f.edi);
    detail.push_str(" EBP=");
    detail.push_hex(f.ebp);

    if vec == 14 {
        detail.push_str("\n  CR2(fault addr)=");
        detail.push_hex(cr2);
        detail.push_str("\n  #PF bits:");
        detail.push_str(if f.error_code & 1 != 0 {
            " present"
        } else {
            " not-present"
        });
        detail.push_str(if f.error_code & 2 != 0 {
            " write"
        } else {
            " read"
        });
        detail.push_str(if f.error_code & 4 != 0 {
            " user"
        } else {
            " supervisor"
        });
        // Signal API: schedule before panic (callback may log; we still halt).
        let _ = signal::schedule_signal(sig::PAGEFAULT);
        signal::process_signals();
    } else if vec == 13 {
        let _ = signal::schedule_signal(sig::GPF);
        signal::process_signals();
    }

    kernel_panic_with_detail("CPU exception", detail.as_str());
}

fn read_cr2() -> u32 {
    let value: u32;
    unsafe {
        asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Fixed-size string builder for panic details (no heap).
struct DetailBuf {
    buf: [u8; 512],
    len: usize,
}

impl DetailBuf {
    fn new() -> Self {
        Self {
            buf: [0; 512],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("<invalid utf8>")
    }

    fn push_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.len >= self.buf.len() {
                return;
            }
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    fn push_u32(&mut self, mut value: u32) {
        if value == 0 {
            self.push_str("0");
            return;
        }
        let mut tmp = [0u8; 10];
        let mut i = 0;
        while value > 0 {
            tmp[i] = b'0' + (value % 10) as u8;
            value /= 10;
            i += 1;
        }
        while i > 0 {
            i -= 1;
            if self.len < self.buf.len() {
                self.buf[self.len] = tmp[i];
                self.len += 1;
            }
        }
    }

    fn push_hex(&mut self, value: u32) {
        self.push_str("0x");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for shift in (0..32).step_by(4).rev() {
            let nibble = ((value >> shift) & 0xF) as usize;
            if self.len < self.buf.len() {
                self.buf[self.len] = HEX[nibble];
                self.len += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ASM stubs are defined in multiboot/exceptions.asm and exported as symbols
// isr_exception_0 ... isr_exception_31.
// ---------------------------------------------------------------------------

extern "C" {
    fn isr_exception_0();
    fn isr_exception_1();
    fn isr_exception_2();
    fn isr_exception_3();
    fn isr_exception_4();
    fn isr_exception_5();
    fn isr_exception_6();
    fn isr_exception_7();
    fn isr_exception_8();
    fn isr_exception_9();
    fn isr_exception_10();
    fn isr_exception_11();
    fn isr_exception_12();
    fn isr_exception_13();
    fn isr_exception_14();
    fn isr_exception_15();
    fn isr_exception_16();
    fn isr_exception_17();
    fn isr_exception_18();
    fn isr_exception_19();
    fn isr_exception_20();
    fn isr_exception_21();
    fn isr_exception_22();
    fn isr_exception_23();
    fn isr_exception_24();
    fn isr_exception_25();
    fn isr_exception_26();
    fn isr_exception_27();
    fn isr_exception_28();
    fn isr_exception_29();
    fn isr_exception_30();
    fn isr_exception_31();
}

/// Install IDT gates for CPU exceptions 0–31.
pub fn init_exceptions() {
    let handlers: [unsafe extern "C" fn(); 32] = [
        isr_exception_0,
        isr_exception_1,
        isr_exception_2,
        isr_exception_3,
        isr_exception_4,
        isr_exception_5,
        isr_exception_6,
        isr_exception_7,
        isr_exception_8,
        isr_exception_9,
        isr_exception_10,
        isr_exception_11,
        isr_exception_12,
        isr_exception_13,
        isr_exception_14,
        isr_exception_15,
        isr_exception_16,
        isr_exception_17,
        isr_exception_18,
        isr_exception_19,
        isr_exception_20,
        isr_exception_21,
        isr_exception_22,
        isr_exception_23,
        isr_exception_24,
        isr_exception_25,
        isr_exception_26,
        isr_exception_27,
        isr_exception_28,
        isr_exception_29,
        isr_exception_30,
        isr_exception_31,
    ];

    for (vector, handler) in handlers.iter().enumerate() {
        register_interrupt_handler(vector as u8, *handler);
    }
}
