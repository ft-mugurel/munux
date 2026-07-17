//! Memory subsystem:
//! - step 2: physical frames (PMM)
//! - step 3: paging / virtual mappings
//! - step 4: kernel heap (kmalloc / virtual alloc)

pub mod heap;
pub mod multiboot;
pub mod paging;
pub mod pmm;

pub use pmm::{
    alloc_frame, frame_size, free_frame, free_frames, init as init_pmm, is_initialized,
    phys_alloc, phys_free, phys_size, total_frames, used_frames, PhysAddr, FRAME_SIZE,
    KERNEL_LOAD_BASE,
};

pub use paging::{
    create_page, get_page, init as init_paging, is_enabled as paging_enabled, map_page,
    page_directory_phys, unmap_page, virt_to_phys, PageInfo, KERNEL_SPACE_START,
    PAGE_KERNEL_RO, PAGE_KERNEL_RW, PAGE_PRESENT, PAGE_USER, PAGE_USER_RO, PAGE_USER_RW,
    PAGE_WRITABLE, USER_SPACE_END, USER_SPACE_START,
};

pub use heap::{
    heap_alloc_count, heap_end, heap_start, heap_used_bytes, init as init_heap, is_initialized as heap_initialized,
    kfree, kmalloc, ksize, virt_alloc, virt_free, virt_size, KERNEL_HEAP_MAX, KERNEL_HEAP_START,
};
