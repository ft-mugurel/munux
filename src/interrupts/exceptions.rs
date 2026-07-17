//! CPU exception handlers (vectors 0–31) for x86_64.

use core::arch::asm;

use crate::interrupts::idt::register_interrupt_handler;

/// Stack layout built by `multiboot/exceptions.asm` after saving GPRs.
///
/// `frame` points at the lowest address (saved RAX).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExceptionFrame {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

const EXCEPTION_NAMES: [&str; 32] = [
    "Divide-by-zero (#DE)",
    "Debug (#DB)",
    "NMI",
    "Breakpoint (#BP)",
    "Overflow (#OF)",
    "Bound range (#BR)",
    "Invalid opcode (#UD)",
    "Device not available (#NM)",
    "Double fault (#DF)",
    "Coprocessor segment overrun",
    "Invalid TSS (#TS)",
    "Segment not present (#NP)",
    "Stack fault (#SS)",
    "General protection (#GP)",
    "Page fault (#PF)",
    "Reserved",
    "x87 FP (#MF)",
    "Alignment check (#AC)",
    "Machine check (#MC)",
    "SIMD FP (#XM)",
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
    "Security (#SX)",
    "Reserved",
];

/// Called from assembly for every CPU exception. Does not return.
#[no_mangle]
pub unsafe extern "C" fn exception_handler(frame: *const ExceptionFrame) -> ! {
    let f = &*frame;
    let vec = f.vector as usize;
    let name = if vec < EXCEPTION_NAMES.len() {
        EXCEPTION_NAMES[vec]
    } else {
        "Unknown exception"
    };

    let cr2 = if vec == 14 { read_cr2() } else { 0 };

    crate::vga_print::clear_screen();
    crate::vga_print::println_line(0, b"*** munux KERNEL PANIC ***", 0x4F);
    crate::vga_print::println_line(1, b"CPU exception", 0x0C);

    let mut line = 3;
    crate::vga_print::print_str(line, 0, name.as_bytes(), 0x0F);
    line += 1;

    crate::vga_print::print_str(line, 0, b"vector=", 0x07);
    crate::vga_print::print_u64(line, 7, f.vector, 0x0E);
    crate::vga_print::print_str(line, 26, b" error=", 0x07);
    crate::vga_print::print_hex64(line, 33, f.error_code, 0x0E);
    line += 1;

    crate::vga_print::print_str(line, 0, b"RIP=", 0x07);
    crate::vga_print::print_hex64(line, 4, f.rip, 0x0B);
    line += 1;

    crate::vga_print::print_str(line, 0, b"CS=", 0x07);
    crate::vga_print::print_hex64(line, 3, f.cs, 0x07);
    crate::vga_print::print_str(line, 22, b" RFLAGS=", 0x07);
    crate::vga_print::print_hex64(line, 30, f.rflags, 0x07);
    line += 1;

    crate::vga_print::print_str(line, 0, b"RSP=", 0x07);
    crate::vga_print::print_hex64(line, 4, f.rsp, 0x07);
    crate::vga_print::print_str(line, 22, b" SS=", 0x07);
    crate::vga_print::print_hex64(line, 26, f.ss, 0x07);
    line += 1;

    crate::vga_print::print_str(line, 0, b"RAX=", 0x07);
    crate::vga_print::print_hex64(line, 4, f.rax, 0x07);
    line += 1;
    crate::vga_print::print_str(line, 0, b"RBX=", 0x07);
    crate::vga_print::print_hex64(line, 4, f.rbx, 0x07);
    line += 1;
    crate::vga_print::print_str(line, 0, b"RCX=", 0x07);
    crate::vga_print::print_hex64(line, 4, f.rcx, 0x07);
    line += 1;
    crate::vga_print::print_str(line, 0, b"RDX=", 0x07);
    crate::vga_print::print_hex64(line, 4, f.rdx, 0x07);
    line += 1;

    if vec == 14 {
        crate::vga_print::print_str(line, 0, b"CR2=", 0x07);
        crate::vga_print::print_hex64(line, 4, cr2, 0x0C);
        line += 1;
        let bits = f.error_code;
        crate::vga_print::print_str(
            line,
            0,
            if bits & 1 != 0 {
                b"#PF: present"
            } else {
                b"#PF: not-present"
            },
            0x0C,
        );
    }

    let _ = line;
    crate::vga_print::println_line(22, b"System halted.", 0x08);

    loop {
        asm!("cli; hlt", options(nomem, nostack));
    }
}

fn read_cr2() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

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
        // Double fault (#DF = 8) uses IST1 for a known-good stack.
        if vector == 8 {
            crate::interrupts::idt::register_gate(vector as u8, *handler, 1);
        } else {
            register_interrupt_handler(vector as u8, *handler);
        }
    }
}
