//! Memory subsystem (x86_64): Multiboot2 map, PMM, 4-level paging, heap.

pub mod heap;
pub mod multiboot;
pub mod paging;
pub mod pmm;

pub use heap::{
    heap_alloc_count, heap_end, heap_start, heap_used_bytes, init as init_heap,
    is_initialized as heap_initialized, kfree, kmalloc, ksize, KERNEL_HEAP_MAX, KERNEL_HEAP_START,
};
pub use multiboot::MULTIBOOT2_MAGIC;
pub use pmm::{
    alloc_frame, free_frame, free_frames, init as init_pmm, is_initialized as pmm_initialized,
    total_frames, used_frames, PhysAddr, FRAME_SIZE,
};
pub use paging::{
    clone_mm, create_page, current_cr3, free_mm, init as init_paging, is_enabled as paging_enabled,
    kernel_cr3, map_page, map_page_in, page_directory_phys, switch_mm, unmap_page, virt_to_phys,
    virt_to_phys_in, PAGE_KERNEL_MMIO, PAGE_KERNEL_RW, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE,
};
