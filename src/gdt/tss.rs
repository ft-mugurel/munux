//! Task State Segment — required for ring-3 → ring-0 stack switch (syscalls / IRQs).

use core::arch::asm;
use core::ptr::addr_of;

use super::gdt::{GdtEntry, GDT_ADDRESS, KERNEL_STACK_SELECTOR};

/// GDT index 7 → selector 0x38
pub const TSS_SELECTOR: u16 = 0x38;

#[repr(C, packed)]
pub struct Tss {
    pub link: u32,
    pub esp0: u32,
    pub ss0: u32,
    pub esp1: u32,
    pub ss1: u32,
    pub esp2: u32,
    pub ss2: u32,
    pub cr3: u32,
    pub eip: u32,
    pub eflags: u32,
    pub eax: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebx: u32,
    pub esp: u32,
    pub ebp: u32,
    pub esi: u32,
    pub edi: u32,
    pub es: u32,
    pub cs: u32,
    pub ss: u32,
    pub ds: u32,
    pub fs: u32,
    pub gs: u32,
    pub ldt: u32,
    pub trap: u16,
    pub iomap_base: u16,
}

static mut TSS: Tss = Tss {
    link: 0,
    esp0: 0,
    ss0: 0,
    esp1: 0,
    ss1: 0,
    esp2: 0,
    ss2: 0,
    cr3: 0,
    eip: 0,
    eflags: 0,
    eax: 0,
    ecx: 0,
    edx: 0,
    ebx: 0,
    esp: 0,
    ebp: 0,
    esi: 0,
    edi: 0,
    es: 0,
    cs: 0,
    ss: 0,
    ds: 0,
    fs: 0,
    gs: 0,
    ldt: 0,
    trap: 0,
    iomap_base: 104, // sizeof Tss
};

extern "C" {
    static stack_top: u8;
}

/// Build a 32-bit available TSS descriptor (type 0x89).
pub fn tss_gdt_entry(base: u32, limit: u32) -> GdtEntry {
    GdtEntry {
        limit_low: (limit & 0xFFFF) as u16,
        base_low: (base & 0xFFFF) as u16,
        base_middle: ((base >> 16) & 0xFF) as u8,
        access: 0x89, // present, ring0, 32-bit TSS available
        granularity: ((limit >> 16) & 0x0F) as u8, // G=0, limit high nibble
        base_high: ((base >> 24) & 0xFF) as u8,
    }
}

/// Initialize TSS (ESP0/SS0) and write descriptor into GDT[7], then `ltr`.
pub fn init_tss() {
    let base = addr_of!(TSS) as u32;
    let limit = (core::mem::size_of::<Tss>() - 1) as u32;
    unsafe {
        TSS.ss0 = KERNEL_STACK_SELECTOR as u32;
        TSS.esp0 = addr_of!(stack_top) as u32;
        TSS.iomap_base = core::mem::size_of::<Tss>() as u16;

        // Patch live GDT at 0x800, entry index 7
        let entry = tss_gdt_entry(base, limit);
        let dest = (GDT_ADDRESS as *mut GdtEntry).add(7);
        dest.write_volatile(entry);

        asm!(
            "mov ax, {sel}",
            "ltr ax",
            sel = const TSS_SELECTOR,
            options(nostack, preserves_flags)
        );
    }
}

/// Update kernel stack pointer used on ring transitions into ring 0.
pub fn set_kernel_stack(esp0: u32) {
    unsafe {
        TSS.esp0 = esp0;
    }
}

pub fn tss_esp0() -> u32 {
    unsafe { core::ptr::addr_of!(TSS.esp0).read_unaligned() }
}
