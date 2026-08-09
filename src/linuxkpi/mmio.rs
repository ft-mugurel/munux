//! ioremap / dma_alloc_coherent (identity + 4K MMIO maps).

use crate::memory::{
    current_cr3, kernel_cr3, map_page_in, virt_to_phys_in, PhysAddr, FRAME_SIZE, PAGE_KERNEL_MMIO,
};

fn map_mmio_range(phys: u64, size: u64) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    let start = phys & !(FRAME_SIZE as u64 - 1);
    let end = phys.saturating_add(size).saturating_add(FRAME_SIZE as u64 - 1)
        & !(FRAME_SIZE as u64 - 1);
    let k = kernel_cr3();
    let cur = current_cr3();
    if k == 0 {
        return core::ptr::null_mut();
    }
    let mut v = start;
    while v < end {
        if virt_to_phys_in(k, v).is_none() {
            map_page_in(k, v, PhysAddr::new(v), PAGE_KERNEL_MMIO);
        }
        if cur != 0 && cur != k && virt_to_phys_in(cur, v).is_none() {
            map_page_in(cur, v, PhysAddr::new(v), PAGE_KERNEL_MMIO);
        }
        v = v.wrapping_add(FRAME_SIZE as u64);
        if v == 0 {
            break;
        }
    }
    phys as *mut u8
}

pub extern "C" fn ioremap(phys: u64, size: u64) -> *mut u8 {
    map_mmio_range(phys, size)
}

pub extern "C" fn iounmap(_addr: *mut u8) {}

/// `void *dma_alloc_coherent(size, dma_addr_t *handle, gfp)` — one identity frame.
pub extern "C" fn dma_alloc_coherent(size: u64, handle: *mut u64, _gfp: u32) -> *mut u8 {
    if size == 0 || size > FRAME_SIZE as u64 {
        return core::ptr::null_mut();
    }
    let Some(frame) = crate::memory::alloc_frame() else {
        return core::ptr::null_mut();
    };
    let phys = frame.as_u64();
    // Low frames are identity-mapped in the 1 GiB window.
    if phys >= 1024 * 1024 * 1024 {
        crate::memory::free_frame(frame);
        return core::ptr::null_mut();
    }
    if !handle.is_null() {
        unsafe {
            *handle = phys;
        }
    }
    unsafe {
        core::ptr::write_bytes(phys as *mut u8, 0, size as usize);
    }
    phys as *mut u8
}

pub extern "C" fn dma_free_coherent(_size: u64, vaddr: *mut u8, dma: u64) {
    if vaddr.is_null() {
        return;
    }
    crate::memory::free_frame(PhysAddr::new(dma & !0xFFF));
}
