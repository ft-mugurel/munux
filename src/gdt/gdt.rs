//! 64-bit GDT: null, kcode, kdata, udata, ucode, TSS (16-byte).
//!
//! Layout chosen for SYSCALL/SYSRET STAR:
//!   SYSCALL: CS=0x08 SS=0x10
//!   SYSRET:  CS=0x23 SS=0x1B  (STAR_user=0x10 → +16 / +8, RPL=3)

use core::arch::asm;
use core::mem::size_of;
use core::ptr::addr_of;

use super::tss::{self, TssDescriptor};

/// 8-byte slots: 0 null, 1 kcode, 2 kdata, 3 udata, 4 ucode, 5–6 TSS.
pub const GDT_ENTRIES: usize = 7;

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
/// User data with RPL=3 (GDT index 3).
pub const USER_DATA_SELECTOR: u16 = 0x1B;
/// User code with RPL=3 (GDT index 4).
pub const USER_CODE_SELECTOR: u16 = 0x23;
/// TSS selector (index 5 → 0x28).
pub const TSS_GDT_INDEX: usize = 5;

/// STAR[63:48] base for SYSRET (user selectors = base+8 / base+16).
pub const STAR_USER_BASE: u16 = 0x10;
/// STAR[47:32] kernel CS for SYSCALL.
pub const STAR_KERNEL_CS: u16 = 0x08;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GdtEntry {
    pub limit_low: u16,
    pub base_low: u16,
    pub base_middle: u8,
    pub access: u8,
    pub granularity: u8,
    pub base_high: u8,
}

impl GdtEntry {
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            granularity: 0,
            base_high: 0,
        }
    }

    pub const fn kernel_code64() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0x9A,
            granularity: 0xAF,
            base_high: 0,
        }
    }

    pub const fn kernel_data64() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0x92,
            granularity: 0xCF,
            base_high: 0,
        }
    }

    /// User data DPL=3.
    pub const fn user_data64() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0xF2,
            granularity: 0xCF,
            base_high: 0,
        }
    }

    /// User 64-bit code DPL=3, L=1.
    pub const fn user_code64() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0xFA,
            granularity: 0xAF,
            base_high: 0,
        }
    }
}

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

static mut GDT: [GdtEntry; GDT_ENTRIES] = [
    GdtEntry::null(),
    GdtEntry::kernel_code64(),
    GdtEntry::kernel_data64(),
    GdtEntry::user_data64(),
    GdtEntry::user_code64(),
    GdtEntry::null(), // TSS low
    GdtEntry::null(), // TSS high
];

pub fn load_gdt() {
    let ptr = GdtPointer {
        limit: (size_of::<[GdtEntry; GDT_ENTRIES]>() - 1) as u16,
        base: addr_of!(GDT) as u64,
    };

    unsafe {
        asm!(
            "lgdt [{}]",
            in(reg) &ptr,
            options(readonly, nostack, preserves_flags)
        );

        asm!(
            "mov {tmp:x}, {data}",
            "mov ds, {tmp:x}",
            "mov es, {tmp:x}",
            "mov ss, {tmp:x}",
            "mov fs, {tmp:x}",
            "mov gs, {tmp:x}",
            "push {code}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            data = const KERNEL_DATA_SELECTOR,
            code = const KERNEL_CODE_SELECTOR as u64,
            tmp = out(reg) _,
        );
    }
}

pub fn set_tss_descriptor(desc: TssDescriptor) {
    unsafe {
        let low = GdtEntry {
            limit_low: desc.limit_low,
            base_low: desc.base_low,
            base_middle: desc.base_middle,
            access: desc.access,
            granularity: desc.granularity,
            base_high: desc.base_high,
        };
        let high = GdtEntry {
            limit_low: (desc.base_upper & 0xFFFF) as u16,
            base_low: ((desc.base_upper >> 16) & 0xFFFF) as u16,
            base_middle: 0,
            access: 0,
            granularity: 0,
            base_high: 0,
        };
        let base = core::ptr::addr_of_mut!(GDT) as *mut GdtEntry;
        core::ptr::write_volatile(base.add(TSS_GDT_INDEX), low);
        core::ptr::write_volatile(base.add(TSS_GDT_INDEX + 1), high);
    }
    let ptr = GdtPointer {
        limit: (size_of::<[GdtEntry; GDT_ENTRIES]>() - 1) as u16,
        base: addr_of!(GDT) as u64,
    };
    unsafe {
        asm!(
            "lgdt [{}]",
            in(reg) &ptr,
            options(readonly, nostack, preserves_flags)
        );
    }
    let _ = tss::TSS_SELECTOR;
}

pub fn entry_count() -> usize {
    GDT_ENTRIES
}

pub fn gdt_base() -> u64 {
    addr_of!(GDT) as u64
}
