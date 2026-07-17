//! Global Descriptor Table (GDT) for a flat 32-bit protected-mode kernel.
//!
//! Layout (required KFS-style segments):
//!   [0] null
//!   [1] kernel code   selector 0x08
//!   [2] kernel data   selector 0x10
//!   [3] kernel stack  selector 0x18
//!   [4] user code     selector 0x20 (use 0x23 from ring 3: index|RPL=3)
//!   [5] user data     selector 0x28 (use 0x2B from ring 3)
//!   [6] user stack    selector 0x30 (use 0x33 from ring 3)
//!
//! The live table is installed at physical address [`GDT_ADDRESS`] (0x800),
//! which is a common KFS-2 requirement ("declare the GDT to the BIOS" / fixed address).

use core::arch::asm;
use core::mem::size_of;
use core::ptr;

/// Physical / linear address where the GDT is installed (`lgdt` base).
pub const GDT_ADDRESS: u32 = 0x0000_0800;

/// Number of descriptors including the null entry.
pub const GDT_ENTRIES: usize = 8; // + TSS at index 7

// ---------------------------------------------------------------------------
// Segment selectors (index << 3 | TI=0 | RPL)
// ---------------------------------------------------------------------------

/// Kernel code — GDT[1], RPL 0
pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
/// Kernel data — GDT[2], RPL 0
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
/// Kernel stack — GDT[3], RPL 0 (loaded into `SS`)
pub const KERNEL_STACK_SELECTOR: u16 = 0x18;
/// User code — GDT[4] with RPL 3 (for future ring-3 transitions)
pub const USER_CODE_SELECTOR: u16 = 0x23;
/// User data — GDT[5] with RPL 3
pub const USER_DATA_SELECTOR: u16 = 0x2B;
/// User stack — GDT[6] with RPL 3
pub const USER_STACK_SELECTOR: u16 = 0x33;

// Access bytes (P=1, S=1 for code/data):
//   Kernel code  0x9A = 1001_1010  DPL=0 exec/read
//   Kernel data  0x92 = 1001_0010  DPL=0 read/write
//   Kernel stack 0x92 = same type as data (writable); separate *selector* for SS.
//                 (Expand-down stack types need a carefully chosen limit; flat
//                 KFS setups normally use a distinct data descriptor for SS.)
//   User code    0xFA = 1111_1010  DPL=3 exec/read
//   User data    0xF2 = 1111_0010  DPL=3 read/write
//   User stack   0xF2 = DPL=3 writable data, dedicated user SS selector
//
// Granularity 0xCF: G=1 (4KiB units), D/B=1 (32-bit), limit high nibble 0xF
// together with limit_low=0xFFFF → full 4 GiB flat segments (base 0).

const ACCESS_KERNEL_CODE: u8 = 0x9A;
const ACCESS_KERNEL_DATA: u8 = 0x92;
const ACCESS_KERNEL_STACK: u8 = 0x92;
const ACCESS_USER_CODE: u8 = 0xFA;
const ACCESS_USER_DATA: u8 = 0xF2;
const ACCESS_USER_STACK: u8 = 0xF2;
const GRAN_FLAT_32: u8 = 0xCF;

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct GdtEntry {
    pub limit_low: u16,
    pub base_low: u16,
    pub base_middle: u8,
    pub access: u8,
    pub granularity: u8,
    pub base_high: u8,
}

#[repr(C, packed)]
pub struct GdtPointer {
    pub limit: u16,
    pub base: u32,
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

    /// Flat segment: base = 0, limit = 4 GiB (with G=1).
    pub const fn flat(access: u8, granularity: u8) -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access,
            granularity,
            base_high: 0,
        }
    }

    pub const fn base(self) -> u32 {
        (self.base_high as u32) << 24
            | (self.base_middle as u32) << 16
            | (self.base_low as u32)
    }

    pub const fn limit(self) -> u32 {
        ((self.granularity as u32) & 0x0F) << 16 | (self.limit_low as u32)
    }
}

/// Template GDT in kernel image; copied to [`GDT_ADDRESS`] before `lgdt`.
static GDT_TEMPLATE: [GdtEntry; GDT_ENTRIES] = [
    GdtEntry::null(),                              // [0] null
    GdtEntry::flat(ACCESS_KERNEL_CODE, GRAN_FLAT_32),  // [1] kernel code
    GdtEntry::flat(ACCESS_KERNEL_DATA, GRAN_FLAT_32),  // [2] kernel data
    GdtEntry::flat(ACCESS_KERNEL_STACK, GRAN_FLAT_32), // [3] kernel stack
    GdtEntry::flat(ACCESS_USER_CODE, GRAN_FLAT_32),    // [4] user code
    GdtEntry::flat(ACCESS_USER_DATA, GRAN_FLAT_32),    // [5] user data
    GdtEntry::flat(ACCESS_USER_STACK, GRAN_FLAT_32),   // [6] user stack
    GdtEntry::null(),                              // [7] TSS (filled by tss::init_tss)
];

/// Install the GDT at [`GDT_ADDRESS`], load GDTR, reload segment registers.
///
/// After this returns:
/// - `CS` = [`KERNEL_CODE_SELECTOR`]
/// - `DS`/`ES`/`FS`/`GS` = [`KERNEL_DATA_SELECTOR`]
/// - `SS` = [`KERNEL_STACK_SELECTOR`]
///
/// User selectors exist in the table but are not loaded yet (still ring 0 only).
pub fn load_gdt() {
    unsafe {
        install_gdt_at(GDT_ADDRESS);
    }

    let gdt_ptr = GdtPointer {
        limit: (size_of::<[GdtEntry; GDT_ENTRIES]>() - 1) as u16,
        base: GDT_ADDRESS,
    };

    unsafe {
        asm!(
            "lgdt [{}]",
            in(reg) &gdt_ptr,
            options(nostack, preserves_flags)
        );

        // Data segments → kernel data (0x10)
        // Stack segment → kernel stack (0x18)
        // Code segment  → kernel code (0x08) via far return
        asm!(
            "mov ax, {data_sel}",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",

            "mov ax, {stack_sel}",
            "mov ss, ax",

            "push {code_sel}",
            "lea eax, [2f]",
            "push eax",
            "retf",
            "2:",
            data_sel = const KERNEL_DATA_SELECTOR,
            stack_sel = const KERNEL_STACK_SELECTOR,
            code_sel = const KERNEL_CODE_SELECTOR as u32,
            out("eax") _,
        );
    }
}

/// Copy the template descriptors to a fixed linear address (identity-mapped low memory).
unsafe fn install_gdt_at(addr: u32) {
    let dest = addr as *mut GdtEntry;
    // Low memory below 1 MiB is identity-accessible with our flat segments / GRUB setup.
    ptr::copy_nonoverlapping(GDT_TEMPLATE.as_ptr(), dest, GDT_ENTRIES);
}

/// Read back one entry from the installed GDT (for debugging / stack dump tools later).
pub fn read_installed_entry(index: usize) -> Option<GdtEntry> {
    if index >= GDT_ENTRIES {
        return None;
    }
    unsafe {
        Some(ptr::read_volatile((GDT_ADDRESS as *const GdtEntry).add(index)))
    }
}

/// Entry count including null.
pub fn entry_count() -> usize {
    GDT_ENTRIES
}
