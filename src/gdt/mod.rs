//! Global Descriptor Table + Task State Segment (x86_64).

pub mod gdt;
pub mod tss;

pub use gdt::{
    load_gdt, entry_count, KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR, GDT_ENTRIES,
};
pub use tss::{init_tss, set_kernel_stack, TSS_SELECTOR};
