//! 64-bit GDT: null, kernel code, kernel data, TSS (16-byte system descriptor).

use core::arch::asm;
use core::mem::size_of;
use core::ptr::addr_of;

use super::tss::{self, TssDescriptor};

/// Number of 8-byte slots (TSS occupies two).
pub const GDT_ENTRIES: usize = 5;

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
/// TSS selector (index 3 → 0x18). Uses slots 3 and 4.
pub const TSS_GDT_INDEX: usize = 3;

/// Standard 8-byte segment descriptor.
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

    /// 64-bit code: L=1, D=0, present, ring0, executable/readable.
    pub const fn kernel_code64() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0x9A,
            granularity: 0xAF, // G=1, L=1, limit high nibble
            base_high: 0,
        }
    }

    /// Data segment (still used for SS/DS in long mode).
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
}

/// GDTR for long mode: 16-bit limit + 64-bit base.
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Live GDT: 3 normal entries + 1 TSS (2 slots) = 5 × 8 bytes.
static mut GDT: [GdtEntry; GDT_ENTRIES] = [
    GdtEntry::null(),
    GdtEntry::kernel_code64(),
    GdtEntry::kernel_data64(),
    GdtEntry::null(), // TSS low
    GdtEntry::null(), // TSS high
];

/// Install kernel code/data descriptors, then load GDTR and reload segments.
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

        // Reload data segments; CS via far return.
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

/// Write the 16-byte TSS descriptor into GDT slots 3–4.
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
        // Store via raw write (packed / static mut).
        let base = core::ptr::addr_of_mut!(GDT) as *mut GdtEntry;
        core::ptr::write_volatile(base.add(TSS_GDT_INDEX), low);
        core::ptr::write_volatile(base.add(TSS_GDT_INDEX + 1), high);
    }
    // Reload GDTR so CPU sees updated TSS entry.
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
