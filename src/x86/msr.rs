//! Model-specific registers (x86_64).

use core::arch::asm;

pub const IA32_FS_BASE: u32 = 0xC000_0100;
pub const IA32_GS_BASE: u32 = 0xC000_0101;

#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let lo = value as u32;
    let hi = (value >> 32) as u32;
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") lo,
        in("edx") hi,
        options(nostack, preserves_flags)
    );
}

#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
        options(nomem, nostack, preserves_flags)
    );
    ((hi as u64) << 32) | (lo as u64)
}

#[inline]
pub fn set_fs_base(base: u64) {
    unsafe { wrmsr(IA32_FS_BASE, base) }
}

#[inline]
pub fn get_fs_base() -> u64 {
    unsafe { rdmsr(IA32_FS_BASE) }
}

#[inline]
pub fn set_gs_base(base: u64) {
    unsafe { wrmsr(IA32_GS_BASE, base) }
}

#[inline]
pub fn get_gs_base() -> u64 {
    unsafe { rdmsr(IA32_GS_BASE) }
}
