//! 64-bit Task State Segment (RSP0 for privilege transitions).

use core::arch::asm;
use core::ptr::addr_of;

use super::gdt::set_tss_descriptor;

/// Selector for GDT index 3.
pub const TSS_SELECTOR: u16 = 0x18;

/// Hardware TSS layout (Intel SDM).
#[repr(C, packed)]
pub struct Tss {
    reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    reserved1: u64,
    pub ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    pub iomap_base: u16,
}

/// Packed 16-byte TSS descriptor fields (split across two GDT entries).
pub struct TssDescriptor {
    pub limit_low: u16,
    pub base_low: u16,
    pub base_middle: u8,
    pub access: u8,
    pub granularity: u8,
    pub base_high: u8,
    pub base_upper: u32,
}

static mut TSS: Tss = Tss {
    reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    reserved1: 0,
    ist: [0; 7],
    reserved2: 0,
    reserved3: 0,
    iomap_base: 0,
};

/// Dedicated double-fault / IST stacks later; for now one kernel IST page.
#[repr(align(16))]
struct IstStack {
    _bytes: [u8; 4096],
}
static mut IST_STACK: IstStack = IstStack { _bytes: [0; 4096] };

fn tss_descriptor(base: u64, limit: u32) -> TssDescriptor {
    TssDescriptor {
        limit_low: (limit & 0xFFFF) as u16,
        base_low: (base & 0xFFFF) as u16,
        base_middle: ((base >> 16) & 0xFF) as u8,
        access: 0x89, // present, 64-bit available TSS
        granularity: ((limit >> 16) & 0x0F) as u8,
        base_high: ((base >> 24) & 0xFF) as u8,
        base_upper: (base >> 32) as u32,
    }
}

/// Fill TSS, install descriptor, `ltr`.
pub fn init_tss() {
    let base = addr_of!(TSS) as u64;
    let limit = (core::mem::size_of::<Tss>() - 1) as u32;

    unsafe {
        // RSP0: current stack is fine until we enter ring 3.
        let mut rsp: u64;
        asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags));
        TSS.rsp0 = rsp;
        TSS.iomap_base = core::mem::size_of::<Tss>() as u16;

        // IST1: separate stack for double fault later.
        let ist_top = (addr_of!(IST_STACK) as *const u8 as usize + 4096) as u64;
        TSS.ist[0] = ist_top;

        set_tss_descriptor(tss_descriptor(base, limit));

        asm!(
            "ltr {sel:x}",
            sel = in(reg) TSS_SELECTOR,
            options(nostack, preserves_flags)
        );
    }
}

pub fn set_kernel_stack(rsp0: u64) {
    unsafe {
        TSS.rsp0 = rsp0;
    }
}

pub fn tss_rsp0() -> u64 {
    unsafe { core::ptr::addr_of!(TSS.rsp0).read_unaligned() }
}
