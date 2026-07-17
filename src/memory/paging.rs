//! x86 32-bit paging (step 3).
//!
//! Builds a page directory + page tables, identity-maps low memory so the
//! kernel keeps running, then enables CR0.PG.

use core::arch::asm;
use core::ptr::addr_of;

use crate::memory::pmm::{self, PhysAddr, FRAME_SIZE, KERNEL_LOAD_BASE};
use crate::panic::kernel_panic;

// ---------------------------------------------------------------------------
// Address-space policy (kernel vs user) — theoretical rights for the subject
// ---------------------------------------------------------------------------

/// User-space virtual range: [USER_SPACE_START, USER_SPACE_END).
/// (Kernel currently lives identity-mapped in low memory with SUPERVISOR pages.)
pub const USER_SPACE_START: u32 = 0x0000_0000;
pub const USER_SPACE_END: u32 = 0xC000_0000;

/// Kernel-space virtual range: [KERNEL_SPACE_START, 0xFFFF_FFFF].
pub const KERNEL_SPACE_START: u32 = 0xC000_0000;

/// How much physical memory we identity-map (VA == PA).
/// Enough for kernel + early allocations; keeps page-table count small.
const IDENTITY_MAP_BYTES: u32 = 32 * 1024 * 1024; // 32 MiB

// ---------------------------------------------------------------------------
// Page-table entry flags
// ---------------------------------------------------------------------------

pub const PAGE_PRESENT: u32 = 1 << 0;
pub const PAGE_WRITABLE: u32 = 1 << 1;
pub const PAGE_USER: u32 = 1 << 2; // U/S=1 → user may access
// U/S=0 (no PAGE_USER) → supervisor / kernel only

pub const PAGE_KERNEL_RW: u32 = PAGE_PRESENT | PAGE_WRITABLE; // supervisor R/W
pub const PAGE_KERNEL_RO: u32 = PAGE_PRESENT; // supervisor R/O
pub const PAGE_USER_RW: u32 = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
pub const PAGE_USER_RO: u32 = PAGE_PRESENT | PAGE_USER;

const ENTRY_ADDR_MASK: u32 = 0xFFFF_F000;
const ENTRIES_PER_TABLE: usize = 1024;

// ---------------------------------------------------------------------------
// Entry + table types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct PageEntry(u32);

impl PageEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn new(frame_phys: u32, flags: u32) -> Self {
        Self((frame_phys & ENTRY_ADDR_MASK) | (flags & 0xFFF))
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn is_present(self) -> bool {
        self.0 & PAGE_PRESENT != 0
    }

    pub const fn is_writable(self) -> bool {
        self.0 & PAGE_WRITABLE != 0
    }

    pub const fn is_user(self) -> bool {
        self.0 & PAGE_USER != 0
    }

    pub const fn frame_addr(self) -> u32 {
        self.0 & ENTRY_ADDR_MASK
    }

    pub const fn flags(self) -> u32 {
        self.0 & 0xFFF
    }
}

#[repr(C, align(4096))]
struct PageTable {
    entries: [PageEntry; ENTRIES_PER_TABLE],
}

// Active page directory (physical address of the PD frame).
static mut PD_PHYS: u32 = 0;
static mut PAGING_ENABLED: bool = false;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
fn pd_index(virt: u32) -> usize {
    ((virt >> 22) & 0x3FF) as usize
}

#[inline]
fn pt_index(virt: u32) -> usize {
    ((virt >> 12) & 0x3FF) as usize
}

/// Physical address → kernel pointer (valid for identity-mapped region).
#[inline]
unsafe fn phys_as_mut_table(phys: u32) -> *mut PageTable {
    phys as *mut PageTable
}

fn invlpg(virt: u32) {
    unsafe {
        asm!("invlpg [{}]", in(reg) virt as usize, options(nostack, preserves_flags));
    }
}

fn read_cr0() -> u32 {
    let v: u32;
    unsafe {
        asm!("mov {}, cr0", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

fn write_cr0(v: u32) {
    unsafe {
        asm!("mov cr0, {}", in(reg) v, options(nostack, preserves_flags));
    }
}

fn write_cr3(v: u32) {
    unsafe {
        asm!("mov cr3, {}", in(reg) v, options(nostack, preserves_flags));
    }
}

fn read_cr3() -> u32 {
    let v: u32;
    unsafe {
        asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

fn zero_frame(phys: PhysAddr) {
    unsafe {
        core::ptr::write_bytes(phys.as_u32() as *mut u8, 0, FRAME_SIZE);
    }
}

fn alloc_table_frame() -> PhysAddr {
    let frame = pmm::alloc_frame().unwrap_or_else(|| kernel_panic("paging: OOM allocating page table"));
    zero_frame(frame);
    frame
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn is_enabled() -> bool {
    unsafe { PAGING_ENABLED }
}

pub fn page_directory_phys() -> Option<PhysAddr> {
    unsafe {
        if PD_PHYS == 0 {
            None
        } else {
            Some(PhysAddr::new(PD_PHYS))
        }
    }
}

/// Result of looking up a virtual page.
#[derive(Clone, Copy, Debug)]
pub struct PageInfo {
    pub virt: u32,
    pub phys: u32,
    pub present: bool,
    pub writable: bool,
    pub user: bool,
    pub flags: u32,
}

/// Translate virtual address → physical (if present).
pub fn virt_to_phys(virt: u32) -> Option<u32> {
    let info = get_page(virt & ENTRY_ADDR_MASK)?;
    if !info.present {
        return None;
    }
    Some(info.phys | (virt & 0xFFF))
}

/// Get mapping info for the page containing `virt`.
pub fn get_page(virt: u32) -> Option<PageInfo> {
    let pd_phys = unsafe { PD_PHYS };
    if pd_phys == 0 {
        return None;
    }
    let virt_page = virt & ENTRY_ADDR_MASK;
    let pdi = pd_index(virt_page);
    let pti = pt_index(virt_page);

    unsafe {
        let pd = phys_as_mut_table(pd_phys);
        let pde = (*pd).entries[pdi];
        if !pde.is_present() {
            return Some(PageInfo {
                virt: virt_page,
                phys: 0,
                present: false,
                writable: false,
                user: false,
                flags: 0,
            });
        }
        let pt = phys_as_mut_table(pde.frame_addr());
        let pte = (*pt).entries[pti];
        Some(PageInfo {
            virt: virt_page,
            phys: pte.frame_addr(),
            present: pte.is_present(),
            writable: pte.is_writable(),
            user: pte.is_user(),
            flags: pte.flags(),
        })
    }
}

/// Ensure the page table for `virt` exists; return its physical address.
unsafe fn ensure_page_table(virt: u32, table_flags: u32) -> u32 {
    let pd_phys = PD_PHYS;
    if pd_phys == 0 {
        kernel_panic("paging: no page directory");
    }
    let pdi = pd_index(virt);
    let pd = phys_as_mut_table(pd_phys);
    let pde = (*pd).entries[pdi];
    if pde.is_present() {
        // Make sure directory entry has at least the requested privilege bits
        let mut flags = pde.flags() | (table_flags & (PAGE_WRITABLE | PAGE_USER));
        flags |= PAGE_PRESENT;
        (*pd).entries[pdi] = PageEntry::new(pde.frame_addr(), flags);
        return pde.frame_addr();
    }
    let pt_frame = alloc_table_frame();
    (*pd).entries[pdi] = PageEntry::new(pt_frame.as_u32(), table_flags | PAGE_PRESENT);
    pt_frame.as_u32()
}

/// Map one virtual page to one physical frame with `flags`.
pub fn map_page(virt: u32, phys: PhysAddr, flags: u32) {
    if virt & 0xFFF != 0 {
        kernel_panic("paging: map_page virt not aligned");
    }
    if !phys.is_aligned() {
        kernel_panic("paging: map_page phys not aligned");
    }
    unsafe {
        if PD_PHYS == 0 {
            kernel_panic("paging: map_page before init");
        }
        // Page tables that hold user pages need USER|WRITE on the PDE too
        let mut table_flags = PAGE_WRITABLE;
        if flags & PAGE_USER != 0 {
            table_flags |= PAGE_USER;
        }
        let pt_phys = ensure_page_table(virt, table_flags);
        let pt = phys_as_mut_table(pt_phys);
        let pti = pt_index(virt);
        (*pt).entries[pti] = PageEntry::new(phys.as_u32(), flags | PAGE_PRESENT);
        if PAGING_ENABLED {
            invlpg(virt);
        }
    }
}

/// Unmap a virtual page. Does **not** free the physical frame (caller decides).
pub fn unmap_page(virt: u32) {
    if virt & 0xFFF != 0 {
        kernel_panic("paging: unmap_page virt not aligned");
    }
    unsafe {
        if PD_PHYS == 0 {
            return;
        }
        let pd = phys_as_mut_table(PD_PHYS);
        let pdi = pd_index(virt);
        let pde = (*pd).entries[pdi];
        if !pde.is_present() {
            return;
        }
        let pt = phys_as_mut_table(pde.frame_addr());
        let pti = pt_index(virt);
        (*pt).entries[pti] = PageEntry::empty();
        if PAGING_ENABLED {
            invlpg(virt);
        }
    }
}

/// Allocate a free physical frame and map it at `virt` with `flags`.
pub fn create_page(virt: u32, flags: u32) -> PhysAddr {
    let frame = pmm::alloc_frame().unwrap_or_else(|| kernel_panic("paging: create_page OOM"));
    map_page(virt, frame, flags);
    frame
}

/// Identity-map [0, len) with kernel R/W supervisor rights.
fn identity_map_range(len: u32) {
    let mut addr = 0u32;
    while addr < len {
        map_page(addr, PhysAddr::new(addr), PAGE_KERNEL_RW);
        addr = addr.wrapping_add(FRAME_SIZE as u32);
        if addr == 0 {
            break; // wrapped
        }
    }
}

/// Build page tables, identity-map low memory, enable paging.
pub fn init() {
    if !pmm::is_initialized() {
        kernel_panic("paging: PMM must be initialized first");
    }
    unsafe {
        if PAGING_ENABLED {
            return;
        }
    }

    // 1) Allocate page directory
    let pd_frame = alloc_table_frame();
    unsafe {
        PD_PHYS = pd_frame.as_u32();
    }

    // 2) Identity-map enough RAM for kernel + early PMM allocations
    let mut map_len = IDENTITY_MAP_BYTES;
    let managed = (pmm::total_frames() as u32).saturating_mul(FRAME_SIZE as u32);
    if managed != 0 && managed < map_len {
        map_len = managed;
    }
    // Always cover at least through kernel_end + margin
    extern "C" {
        static kernel_end: u8;
    }
    let kend = align_up_u32(addr_of!(kernel_end) as u32, FRAME_SIZE as u32);
    let min_map = kend.saturating_add(4 * 1024 * 1024); // +4 MiB slack
    if map_len < min_map {
        map_len = min_map;
    }

    identity_map_range(map_len);

    // 3) Enable paging
    unsafe {
        write_cr3(PD_PHYS);
        let mut cr0 = read_cr0();
        cr0 |= 1 << 31; // PG
        // PE should already be set; keep it
        cr0 |= 1; // PE
        write_cr0(cr0);

        // Flush TLB by reloading CR3
        write_cr3(PD_PHYS);

        PAGING_ENABLED = true;
    }

    // Sanity: identity translation of kernel entry region
    let sample = KERNEL_LOAD_BASE;
    match virt_to_phys(sample) {
        Some(p) if p == sample => {}
        other => {
            let _ = other;
            kernel_panic("paging: identity map self-check failed");
        }
    }

    crate::println!(
        "paging: ON  CR3={:#010x}  identity 0..{:#x} ({} MiB)",
        page_directory_phys().map(|p| p.as_u32()).unwrap_or(0),
        map_len,
        map_len / (1024 * 1024)
    );
    crate::println!(
        "paging: user VA [{:#x}, {:#x})  kernel VA >= {:#x}",
        USER_SPACE_START,
        USER_SPACE_END,
        KERNEL_SPACE_START
    );
}

fn align_up_u32(addr: u32, align: u32) -> u32 {
    (addr + align - 1) & !(align - 1)
}

/// Returns whether `virt` lies in the documented user region.
pub fn is_user_address(virt: u32) -> bool {
    virt >= USER_SPACE_START && virt < USER_SPACE_END
}

/// Returns whether `virt` lies in the documented kernel region.
pub fn is_kernel_address(virt: u32) -> bool {
    virt >= KERNEL_SPACE_START
}

/// Read CR0 / CR3 for debugging.
pub fn debug_cr0() -> u32 {
    read_cr0()
}

pub fn debug_cr3() -> u32 {
    read_cr3()
}
