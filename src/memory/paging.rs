//! x86_64 4-level paging.
//!
//! Builds a new PML4 and identity-maps low physical memory with 2 MiB pages,
//! then loads CR3. Relies on the trampoline already having enabled long mode
//! and identity-mapped at least the kernel so this code can run.

use core::arch::asm;
use core::ptr::addr_of;

use crate::memory::pmm::{self, PhysAddr, FRAME_SIZE, KERNEL_LOAD_BASE};

pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_SIZE_2M: u64 = 1 << 7; // PS bit in PD entry
pub const PAGE_KERNEL_RW: u64 = PAGE_PRESENT | PAGE_WRITABLE;

const ENTRY_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const ENTRIES: usize = 512;

/// How much to identity-map with 2 MiB pages (must cover kernel + early allocs).
const IDENTITY_MAP_BYTES: u64 = 1 * 1024 * 1024 * 1024; // 1 GiB

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Entry(u64);

impl Entry {
    const fn empty() -> Self {
        Self(0)
    }

    const fn new(phys: u64, flags: u64) -> Self {
        Self((phys & ENTRY_ADDR_MASK) | (flags & 0xFFF))
    }

    const fn is_present(self) -> bool {
        self.0 & PAGE_PRESENT != 0
    }

    const fn addr(self) -> u64 {
        self.0 & ENTRY_ADDR_MASK
    }

    const fn flags(self) -> u64 {
        self.0 & 0xFFF
    }
}

#[repr(C, align(4096))]
struct Table {
    entries: [Entry; ENTRIES],
}

static mut PML4_PHYS: u64 = 0;
static mut PAGING_ENABLED: bool = false;

fn pml4_index(virt: u64) -> usize {
    ((virt >> 39) & 0x1FF) as usize
}
fn pdpt_index(virt: u64) -> usize {
    ((virt >> 30) & 0x1FF) as usize
}
fn pd_index(virt: u64) -> usize {
    ((virt >> 21) & 0x1FF) as usize
}
fn pt_index(virt: u64) -> usize {
    ((virt >> 12) & 0x1FF) as usize
}

unsafe fn table_mut(phys: u64) -> *mut Table {
    // Identity map assumed for page-table frames we allocate.
    phys as *mut Table
}

fn zero_frame(phys: PhysAddr) {
    unsafe {
        core::ptr::write_bytes(phys.as_u64() as *mut u8, 0, FRAME_SIZE);
    }
}

fn alloc_table() -> PhysAddr {
    let f = pmm::alloc_frame().expect("paging: OOM page table");
    zero_frame(f);
    f
}

fn write_cr3(v: u64) {
    unsafe {
        asm!("mov cr3, {}", in(reg) v, options(nostack, preserves_flags));
    }
}

fn read_cr3() -> u64 {
    let v: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

fn invlpg(virt: u64) {
    unsafe {
        asm!("invlpg [{}]", in(reg) virt as usize, options(nostack, preserves_flags));
    }
}

pub fn is_enabled() -> bool {
    unsafe { PAGING_ENABLED }
}

pub fn page_directory_phys() -> Option<PhysAddr> {
    unsafe {
        if PML4_PHYS == 0 {
            None
        } else {
            Some(PhysAddr::new(PML4_PHYS))
        }
    }
}

/// Translate virt -> phys if mapped (4K leaf or 2M page).
pub fn virt_to_phys(virt: u64) -> Option<u64> {
    let pml4 = unsafe { PML4_PHYS };
    if pml4 == 0 {
        return None;
    }
    unsafe {
        let e4 = (*table_mut(pml4)).entries[pml4_index(virt)];
        if !e4.is_present() {
            return None;
        }
        let e3 = (*table_mut(e4.addr())).entries[pdpt_index(virt)];
        if !e3.is_present() {
            return None;
        }
        let e2 = (*table_mut(e3.addr())).entries[pd_index(virt)];
        if !e2.is_present() {
            return None;
        }
        if e2.flags() & PAGE_SIZE_2M != 0 {
            // 2 MiB page
            let base = e2.addr() & !0x1F_FFFF;
            return Some(base | (virt & 0x1F_FFFF));
        }
        let e1 = (*table_mut(e2.addr())).entries[pt_index(virt)];
        if !e1.is_present() {
            return None;
        }
        Some(e1.addr() | (virt & 0xFFF))
    }
}

/// Map one 4 KiB page (creates intermediate tables as needed).
pub fn map_page(virt: u64, phys: PhysAddr, flags: u64) {
    assert!(virt % FRAME_SIZE as u64 == 0);
    assert!(phys.is_aligned());
    let pml4 = unsafe {
        if PML4_PHYS == 0 {
            panic_paging("map_page before init");
        }
        PML4_PHYS
    };

    unsafe {
        // Privilege bits that intermediate tables must allow for user pages.
        let need = PAGE_PRESENT | PAGE_WRITABLE | (flags & PAGE_USER);

        // PML4 -> PDPT
        let t4 = table_mut(pml4);
        let i4 = pml4_index(virt);
        if !(*t4).entries[i4].is_present() {
            let pdpt = alloc_table();
            (*t4).entries[i4] = Entry::new(pdpt.as_u64(), need);
        } else {
            // Upgrade U/S if mapping a user page through an existing kernel PDE path
            let e = (*t4).entries[i4];
            (*t4).entries[i4] = Entry::new(e.addr(), e.flags() | need);
        }
        let e4 = (*t4).entries[i4];

        // PDPT -> PD
        let t3 = table_mut(e4.addr());
        let i3 = pdpt_index(virt);
        if !(*t3).entries[i3].is_present() {
            let pd = alloc_table();
            (*t3).entries[i3] = Entry::new(pd.as_u64(), need);
        } else {
            let e = (*t3).entries[i3];
            (*t3).entries[i3] = Entry::new(e.addr(), e.flags() | need);
        }
        let e3 = (*t3).entries[i3];

        // PD -> PT (split 2 MiB pages into 4 KiB tables if needed)
        let t2 = table_mut(e3.addr());
        let i2 = pd_index(virt);
        let e2 = (*t2).entries[i2];
        if e2.is_present() && e2.flags() & PAGE_SIZE_2M != 0 {
            // Expand huge page so we can set per-4K USER/flags (ELF at 0x400000).
            let base = e2.addr() & !0x1F_FFFF;
            let pt = alloc_table();
            let pt_t = table_mut(pt.as_u64());
            for i in 0..ENTRIES {
                let phys = base + (i as u64) * FRAME_SIZE as u64;
                // Preserve identity (supervisor R/W); leaf may upgrade USER below.
                (*pt_t).entries[i] = Entry::new(phys, PAGE_PRESENT | PAGE_WRITABLE);
            }
            (*t2).entries[i2] = Entry::new(pt.as_u64(), need);
        } else if !e2.is_present() {
            let pt = alloc_table();
            (*t2).entries[i2] = Entry::new(pt.as_u64(), need);
        } else {
            let e = (*t2).entries[i2];
            (*t2).entries[i2] = Entry::new(e.addr(), e.flags() | need);
        }
        let e2 = (*t2).entries[i2];

        let t1 = table_mut(e2.addr());
        let i1 = pt_index(virt);
        (*t1).entries[i1] = Entry::new(phys.as_u64(), flags | PAGE_PRESENT);
        if PAGING_ENABLED {
            invlpg(virt);
        }
    }
}

/// Allocate a frame and map it at `virt`.
pub fn create_page(virt: u64, flags: u64) -> PhysAddr {
    let frame = pmm::alloc_frame().expect("paging: create_page OOM");
    map_page(virt, frame, flags);
    frame
}

pub fn unmap_page(virt: u64) {
    if virt % FRAME_SIZE as u64 != 0 {
        return;
    }
    let pml4 = unsafe { PML4_PHYS };
    if pml4 == 0 {
        return;
    }
    unsafe {
        let e4 = (*table_mut(pml4)).entries[pml4_index(virt)];
        if !e4.is_present() {
            return;
        }
        let e3 = (*table_mut(e4.addr())).entries[pdpt_index(virt)];
        if !e3.is_present() {
            return;
        }
        let e2 = (*table_mut(e3.addr())).entries[pd_index(virt)];
        if !e2.is_present() || e2.flags() & PAGE_SIZE_2M != 0 {
            return;
        }
        let t1 = table_mut(e2.addr());
        (*t1).entries[pt_index(virt)] = Entry::empty();
        if PAGING_ENABLED {
            invlpg(virt);
        }
    }
}

/// Identity-map [0, len) using 2 MiB pages (fast, few tables).
fn identity_map_2m(len: u64) {
    let pml4 = unsafe { PML4_PHYS };
    let mut addr = 0u64;
    while addr < len {
        let i4 = pml4_index(addr);
        let i3 = pdpt_index(addr);
        let i2 = pd_index(addr);

        unsafe {
            let t4 = table_mut(pml4);
            if !(*t4).entries[i4].is_present() {
                let pdpt = alloc_table();
                (*t4).entries[i4] = Entry::new(pdpt.as_u64(), PAGE_PRESENT | PAGE_WRITABLE);
            }
            let e4 = (*t4).entries[i4];

            let t3 = table_mut(e4.addr());
            if !(*t3).entries[i3].is_present() {
                let pd = alloc_table();
                (*t3).entries[i3] = Entry::new(pd.as_u64(), PAGE_PRESENT | PAGE_WRITABLE);
            }
            let e3 = (*t3).entries[i3];

            let t2 = table_mut(e3.addr());
            // 2 MiB page: phys = addr, PS|P|RW
            (*t2).entries[i2] = Entry::new(addr, PAGE_PRESENT | PAGE_WRITABLE | PAGE_SIZE_2M);
        }

        addr = addr.saturating_add(2 * 1024 * 1024);
        if addr == 0 {
            break;
        }
    }
}

/// Build kernel page tables and switch CR3.
pub fn init() {
    if !pmm::is_initialized() {
        panic_paging("PMM must init first");
    }
    unsafe {
        if PAGING_ENABLED && PML4_PHYS != 0 {
            return;
        }
    }

    let pml4 = alloc_table();
    unsafe {
        PML4_PHYS = pml4.as_u64();
    }

    // Map at least identity of early RAM; shrink to managed phys if smaller.
    let mut map_len = IDENTITY_MAP_BYTES;
    let managed = (pmm::total_frames() as u64).saturating_mul(FRAME_SIZE as u64);
    if managed != 0 && managed < map_len {
        map_len = managed;
    }
    // Always cover kernel_end + slack
    extern "C" {
        static kernel_end: u8;
    }
    let kend = (addr_of!(kernel_end) as u64 + FRAME_SIZE as u64 - 1) & !(FRAME_SIZE as u64 - 1);
    let min_map = kend.saturating_add(16 * 1024 * 1024);
    if map_len < min_map {
        map_len = min_map;
    }
    // Align up to 2 MiB
    map_len = (map_len + 0x1F_FFFF) & !0x1F_FFFF;

    identity_map_2m(map_len);

    // Switch to our tables (long mode already on from trampoline)
    write_cr3(pml4.as_u64());
    unsafe {
        PAGING_ENABLED = true;
    }

    // Self-check: kernel load base identity
    match virt_to_phys(KERNEL_LOAD_BASE) {
        Some(p) if p == KERNEL_LOAD_BASE => {}
        _ => panic_paging("identity self-check failed"),
    }

    let _ = read_cr3();
    let _ = map_len;
}

fn panic_paging(msg: &str) -> ! {
    crate::vga_print::clear_screen();
    crate::vga_print::println_line(0, b"*** paging panic ***", 0x4F);
    crate::vga_print::println_line(2, msg.as_bytes(), 0x0F);
    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

/// Bytes identity-mapped at init (for display). Approximate via virt_to_phys scan not needed.
pub fn identity_map_size_hint() -> u64 {
    IDENTITY_MAP_BYTES
}
