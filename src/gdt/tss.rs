//! 64-bit Task State Segment (RSP0 for ring-3 → ring-0).

use core::arch::asm;
use core::ptr::addr_of;

use super::gdt::set_tss_descriptor;

/// GDT index 5 → selector 0x28
pub const TSS_SELECTOR: u16 = 0x28;

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

#[repr(align(16))]
struct IstStack {
    _bytes: [u8; 4096],
}
static mut IST_STACK: IstStack = IstStack { _bytes: [0; 4096] };

/// Dedicated ring-0 stack for syscalls / IRQs from user mode.
#[repr(align(16))]
struct KernelStack {
    _bytes: [u8; 16384],
}
static mut KERNEL_STACK: KernelStack = KernelStack { _bytes: [0; 16384] };

fn tss_descriptor(base: u64, limit: u32) -> TssDescriptor {
    TssDescriptor {
        limit_low: (limit & 0xFFFF) as u16,
        base_low: (base & 0xFFFF) as u16,
        base_middle: ((base >> 16) & 0xFF) as u8,
        access: 0x89,
        granularity: ((limit >> 16) & 0x0F) as u8,
        base_high: ((base >> 24) & 0xFF) as u8,
        base_upper: (base >> 32) as u32,
    }
}

pub fn init_tss() {
    let base = addr_of!(TSS) as u64;
    let limit = (core::mem::size_of::<Tss>() - 1) as u32;

    unsafe {
        let kstack_top =
            (addr_of!(KERNEL_STACK) as *const u8 as usize + core::mem::size_of::<KernelStack>())
                as u64;
        TSS.rsp0 = kstack_top;
        TSS.iomap_base = core::mem::size_of::<Tss>() as u16;

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

pub fn kernel_stack_top() -> u64 {
    (addr_of!(KERNEL_STACK) as *const u8 as usize + core::mem::size_of::<KernelStack>()) as u64
}

pub fn tss_rsp0() -> u64 {
    unsafe { core::ptr::addr_of!(TSS.rsp0).read_unaligned() }
}
