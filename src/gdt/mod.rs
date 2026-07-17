//! Global Descriptor Table + Task State Segment (x86_64).

pub mod gdt;
pub mod tss;

pub use gdt::{
    entry_count, load_gdt, KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR, STAR_KERNEL_CS,
    STAR_USER_BASE, USER_CODE_SELECTOR, USER_DATA_SELECTOR, GDT_ENTRIES,
};
pub use tss::{init_tss, kernel_stack_top, set_kernel_stack, TSS_SELECTOR};
