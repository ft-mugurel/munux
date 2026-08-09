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
pub const PAGE_PWT: u64 = 1 << 3;
pub const PAGE_PCD: u64 = 1 << 4;
pub const PAGE_SIZE_2M: u64 = 1 << 7; // PS bit in PD entry
pub const PAGE_KERNEL_RW: u64 = PAGE_PRESENT | PAGE_WRITABLE;
pub const PAGE_KERNEL_MMIO: u64 = PAGE_PRESENT | PAGE_WRITABLE | PAGE_PWT | PAGE_PCD;

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
/// Boot / reference kernel page tables. Never free; always valid after [`init`].
static mut KERNEL_PML4: u64 = 0;
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

/// Physical address of the boot kernel PML4 (shared kernel mapping root).
///
/// All process CR3 values must keep the same kernel entries as this table
/// (Phase 1: identity window + heap). Returns `0` before [`init`].
pub fn kernel_cr3() -> u64 {
    unsafe { KERNEL_PML4 }
}

/// CR3 currently loaded in the CPU (PML4 physical address).
pub fn current_cr3() -> u64 {
    read_cr3()
}

/// Load process page tables. No-op if `cr3 == 0` or already active.
///
/// Also updates the software “current tables” pointer used by `map_page` /
/// `virt_to_phys` so later mapping ops hit the same tree as the CPU.
pub fn switch_mm(cr3: u64) {
    if cr3 == 0 {
        return;
    }
    unsafe {
        if read_cr3() == cr3 {
            // Keep software view in sync even if hardware already matches.
            PML4_PHYS = cr3;
            return;
        }
        write_cr3(cr3);
        PML4_PHYS = cr3;
    }
}

// ---------------------------------------------------------------------------
// Phase 1: clone / free address spaces
// ---------------------------------------------------------------------------

/// Create a new address space that mirrors `src_cr3`.
///
/// - **Kernel leaves** (no `PAGE_USER`): shared physical frames.
/// - **User leaves** (`PAGE_USER`): new frames + content copy (isolation).
/// - 2 MiB user pages are split to 4 KiB and copied.
///
/// Returns the new PML4 physical address, or `None` on OOM.
pub fn clone_mm(src_cr3: u64) -> Option<u64> {
    if src_cr3 == 0 {
        return None;
    }
    let dst = alloc_table();
    if !clone_pml4(src_cr3, dst.as_u64(), 0) {
        free_mm(dst.as_u64());
        return None;
    }
    Some(dst.as_u64())
}

/// Tear down a process address space created by [`clone_mm`].
///
/// - Never free [`kernel_cr3`].
/// - Free **private user** leaf frames (`PAGE_USER` and `phys != virt` page base).
/// - Free all intermediate page-table pages for this tree.
pub fn free_mm(cr3: u64) {
    if cr3 == 0 {
        return;
    }
    let k = unsafe { KERNEL_PML4 };
    if cr3 == k {
        return;
    }
    if current_cr3() == cr3 {
        if k != 0 {
            switch_mm(k);
        }
    }
    free_pml4_tree(cr3);
    pmm::free_frame(PhysAddr::new(cr3));
}

fn clone_pml4(src: u64, dst: u64, va_base: u64) -> bool {
    unsafe {
        let s = table_mut(src);
        let d = table_mut(dst);
        for i in 0..ENTRIES {
            let e = (*s).entries[i];
            if !e.is_present() {
                continue;
            }
            let va = va_base | ((i as u64) << 39);
            let Some(new_pdpt) = alloc_table_opt() else {
                return false;
            };
            if !clone_pdpt(e.addr(), new_pdpt.as_u64(), va) {
                pmm::free_frame(new_pdpt);
                return false;
            }
            (*d).entries[i] = Entry::new(new_pdpt.as_u64(), e.flags());
        }
    }
    true
}

fn clone_pdpt(src: u64, dst: u64, va_base: u64) -> bool {
    unsafe {
        let s = table_mut(src);
        let d = table_mut(dst);
        for i in 0..ENTRIES {
            let e = (*s).entries[i];
            if !e.is_present() {
                continue;
            }
            let va = va_base | ((i as u64) << 30);
            let Some(new_pd) = alloc_table_opt() else {
                return false;
            };
            if !clone_pd(e.addr(), new_pd.as_u64(), va) {
                pmm::free_frame(new_pd);
                return false;
            }
            (*d).entries[i] = Entry::new(new_pd.as_u64(), e.flags());
        }
    }
    true
}

fn clone_pd(src: u64, dst: u64, va_base: u64) -> bool {
    unsafe {
        let s = table_mut(src);
        let d = table_mut(dst);
        for i in 0..ENTRIES {
            let e = (*s).entries[i];
            if !e.is_present() {
                continue;
            }
            let va = va_base | ((i as u64) << 21);
            // 2 MiB leaf
            if e.flags() & PAGE_SIZE_2M != 0 {
                if e.flags() & PAGE_USER != 0 {
                    // Split + copy into private 4K frames.
                    let Some(new_pt) = alloc_table_opt() else {
                        return false;
                    };
                    if !copy_2m_user_to_pt(e.addr() & !0x1F_FFFF, new_pt.as_u64(), e.flags()) {
                        pmm::free_frame(new_pt);
                        return false;
                    }
                    let need = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
                    (*d).entries[i] = Entry::new(new_pt.as_u64(), need);
                } else {
                    // Kernel huge page — share.
                    (*d).entries[i] = e;
                }
                continue;
            }
            let Some(new_pt) = alloc_table_opt() else {
                return false;
            };
            if !clone_pt(e.addr(), new_pt.as_u64(), va) {
                pmm::free_frame(new_pt);
                return false;
            }
            (*d).entries[i] = Entry::new(new_pt.as_u64(), e.flags());
        }
    }
    true
}

fn clone_pt(src: u64, dst: u64, va_base: u64) -> bool {
    unsafe {
        let s = table_mut(src);
        let d = table_mut(dst);
        for i in 0..ENTRIES {
            let e = (*s).entries[i];
            if !e.is_present() {
                (*d).entries[i] = Entry::empty();
                continue;
            }
            if e.flags() & PAGE_USER != 0 {
                // Private copy of user page.
                let Some(nf) = alloc_table_opt() else {
                    // alloc_table_opt zeros; reuse as data frame.
                    return false;
                };
                let src_phys = e.addr();
                core::ptr::copy_nonoverlapping(
                    src_phys as *const u8,
                    nf.as_u64() as *mut u8,
                    FRAME_SIZE,
                );
                (*d).entries[i] = Entry::new(nf.as_u64(), e.flags());
                let _ = va_base; // kept for future COW / debug
            } else {
                // Kernel leaf — share.
                (*d).entries[i] = e;
            }
        }
    }
    true
}

/// Expand a 2 MiB user page into 512 private 4 KiB frames with copied content.
fn copy_2m_user_to_pt(phys_base: u64, pt_phys: u64, src_flags: u64) -> bool {
    let leaf_flags = (src_flags & !PAGE_SIZE_2M) | PAGE_PRESENT | PAGE_USER;
    unsafe {
        let pt = table_mut(pt_phys);
        for i in 0..ENTRIES {
            let Some(nf) = alloc_table_opt() else {
                // Best-effort: free frames we already put in this PT.
                for j in 0..i {
                    let e = (*pt).entries[j];
                    if e.is_present() {
                        pmm::free_frame(PhysAddr::new(e.addr()));
                    }
                }
                return false;
            };
            let src = (phys_base + (i as u64) * FRAME_SIZE as u64) as *const u8;
            core::ptr::copy_nonoverlapping(src, nf.as_u64() as *mut u8, FRAME_SIZE);
            (*pt).entries[i] = Entry::new(nf.as_u64(), leaf_flags);
        }
    }
    true
}

fn alloc_table_opt() -> Option<PhysAddr> {
    let f = pmm::alloc_frame()?;
    zero_frame(f);
    Some(f)
}

/// True if this user leaf is a privately allocated frame (safe to return to PMM).
fn user_frame_is_private(virt: u64, phys: u64) -> bool {
    let p = phys & !0xFFF;
    let v = virt & !0xFFF;
    if p == v {
        return false; // identity-backed — may still be shared kernel RAM
    }
    if p < 0x100000 {
        return false;
    }
    true
}

fn free_pml4_tree(pml4: u64) {
    unsafe {
        let t4 = table_mut(pml4);
        for i4 in 0..ENTRIES {
            let e4 = (*t4).entries[i4];
            if !e4.is_present() {
                continue;
            }
            let va4 = (i4 as u64) << 39;
            free_pdpt_tree(e4.addr(), va4);
            pmm::free_frame(PhysAddr::new(e4.addr()));
            (*t4).entries[i4] = Entry::empty();
        }
    }
}

fn free_pdpt_tree(pdpt: u64, va_base: u64) {
    unsafe {
        let t3 = table_mut(pdpt);
        for i3 in 0..ENTRIES {
            let e3 = (*t3).entries[i3];
            if !e3.is_present() {
                continue;
            }
            let va3 = va_base | ((i3 as u64) << 30);
            free_pd_tree(e3.addr(), va3);
            pmm::free_frame(PhysAddr::new(e3.addr()));
            (*t3).entries[i3] = Entry::empty();
        }
    }
}

fn free_pd_tree(pd: u64, va_base: u64) {
    unsafe {
        let t2 = table_mut(pd);
        for i2 in 0..ENTRIES {
            let e2 = (*t2).entries[i2];
            if !e2.is_present() {
                continue;
            }
            let va2 = va_base | ((i2 as u64) << 21);
            if e2.flags() & PAGE_SIZE_2M != 0 {
                // Shared kernel 2M or (should not happen) user 2M — never free leaf.
                (*t2).entries[i2] = Entry::empty();
                continue;
            }
            free_pt_leaves(e2.addr(), va2);
            pmm::free_frame(PhysAddr::new(e2.addr()));
            (*t2).entries[i2] = Entry::empty();
        }
    }
}

fn free_pt_leaves(pt: u64, va_base: u64) {
    unsafe {
        let t1 = table_mut(pt);
        for i1 in 0..ENTRIES {
            let e1 = (*t1).entries[i1];
            if !e1.is_present() {
                continue;
            }
            let va = va_base | ((i1 as u64) << 12);
            if e1.flags() & PAGE_USER != 0 && user_frame_is_private(va, e1.addr()) {
                pmm::free_frame(PhysAddr::new(e1.addr()));
            }
            (*t1).entries[i1] = Entry::empty();
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

/// Map one 4 KiB page in the **current** address space ([`PML4_PHYS`]).
pub fn map_page(virt: u64, phys: PhysAddr, flags: u64) {
    let pml4 = unsafe {
        if PML4_PHYS == 0 {
            panic_paging("map_page before init");
        }
        PML4_PHYS
    };
    map_page_in(pml4, virt, phys, flags);
}

/// Map one 4 KiB page into a specific page-table root (may differ from active CR3).
///
/// Used by fork to install the child stack into `child_cr3` without switching
/// the CPU away from the parent. Does not shoot down the other CR3's TLB; the
/// child loads CR3 later via [`switch_mm`].
pub fn map_page_in(pml4: u64, virt: u64, phys: PhysAddr, flags: u64) {
    assert!(virt % FRAME_SIZE as u64 == 0);
    assert!(phys.is_aligned());
    if pml4 == 0 {
        panic_paging("map_page_in: null pml4");
    }

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
            let base = e2.0 & !0x1F_FFFF;
            let leaf_flags = PAGE_PRESENT
                | PAGE_WRITABLE
                | (e2.flags() & PAGE_USER)
                | (flags & PAGE_USER);
            let pt = alloc_table();
            let pt_t = table_mut(pt.as_u64());
            for i in 0..ENTRIES {
                let p = base + (i as u64) * FRAME_SIZE as u64;
                (*pt_t).entries[i] = Entry::new(p, leaf_flags);
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
        if e2.flags() & PAGE_SIZE_2M != 0 {
            panic_paging("map_page_in: PD still PS after split");
        }

        let t1 = table_mut(e2.addr());
        let i1 = pt_index(virt);
        (*t1).entries[i1] = Entry::new(phys.as_u64(), flags | PAGE_PRESENT);

        // Only invalidate TLB if this tree is currently loaded.
        if PAGING_ENABLED && read_cr3() == pml4 {
            invlpg(virt);
        }
    }
}

/// Translate virt → phys in a specific address space.
pub fn virt_to_phys_in(pml4: u64, virt: u64) -> Option<u64> {
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

/// Allocate a frame and map it at `virt`.
pub fn create_page(virt: u64, flags: u64) -> PhysAddr {
    let frame = pmm::alloc_frame().expect("paging: create_page OOM");
    map_page(virt, frame, flags);
    frame
}

/// Ensure a 2 MiB identity region is user-accessible (U/S=1) without splitting
/// into 4 KiB pages. Used for classic ET_EXEC loads at 0x400000+.
///
/// `base` must be 2 MiB-aligned. Phys = virt (identity). Claims each 4 KiB
/// frame in the PMM so later `alloc_frame` will not reuse them.
pub fn map_identity_2m_user(base: u64) -> Result<(), &'static str> {
    if base & 0x1F_FFFF != 0 {
        return Err("paging: 2m base unaligned");
    }
    let pml4 = unsafe {
        if PML4_PHYS == 0 {
            return Err("paging: not init");
        }
        PML4_PHYS
    };
    let need = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;
    let flags_2m = need | PAGE_SIZE_2M;

    unsafe {
        let t4 = table_mut(pml4);
        let i4 = pml4_index(base);
        if !(*t4).entries[i4].is_present() {
            let pdpt = alloc_table();
            (*t4).entries[i4] = Entry::new(pdpt.as_u64(), need);
        } else {
            let e = (*t4).entries[i4];
            (*t4).entries[i4] = Entry::new(e.addr(), e.flags() | need);
        }
        let e4 = (*t4).entries[i4];

        let t3 = table_mut(e4.addr());
        let i3 = pdpt_index(base);
        if !(*t3).entries[i3].is_present() {
            let pd = alloc_table();
            (*t3).entries[i3] = Entry::new(pd.as_u64(), need);
        } else {
            let e = (*t3).entries[i3];
            (*t3).entries[i3] = Entry::new(e.addr(), e.flags() | need);
        }
        let e3 = (*t3).entries[i3];

        let t2 = table_mut(e3.addr());
        let i2 = pd_index(base);
        let e2 = (*t2).entries[i2];

        if e2.is_present() && e2.flags() & PAGE_SIZE_2M == 0 {
            // Already split to 4K — set USER on every leaf instead.
            let pt = e2.addr();
            // Upgrade PDE U/S
            (*t2).entries[i2] = Entry::new(pt, e2.flags() | need);
            let pt_t = table_mut(pt);
            for i in 0..ENTRIES {
                let e = (*pt_t).entries[i];
                if e.is_present() {
                    let p = e.addr();
                    (*pt_t).entries[i] = Entry::new(p, PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
                    let _ = pmm::claim_frame(PhysAddr::new(p));
                } else {
                    let p = base + (i as u64) * FRAME_SIZE as u64;
                    (*pt_t).entries[i] = Entry::new(p, PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
                    let _ = pmm::claim_frame(PhysAddr::new(p));
                }
            }
        } else {
            // Install / upgrade 2 MiB identity leaf with USER.
            (*t2).entries[i2] = Entry::new(base, flags_2m);
            let mut off = 0u64;
            while off < 0x20_0000 {
                let _ = pmm::claim_frame(PhysAddr::new(base + off));
                off += FRAME_SIZE as u64;
            }
        }

        if PAGING_ENABLED {
            let mut v = base;
            while v < base + 0x20_0000 {
                invlpg(v);
                v += FRAME_SIZE as u64;
            }
            write_cr3(read_cr3());
        }
    }
    Ok(())
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
        KERNEL_PML4 = pml4.as_u64();
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

