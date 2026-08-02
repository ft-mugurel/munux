//! Per-process kernel stacks (ring 0).
//!
//! Slot 0 (kinit) uses the boot TSS stack from [`crate::gdt::tss`].
//! Other slots get a private 16 KiB stack from a static pool.
//!
//! On task switch we install the stack as:
//! - TSS.RSP0 (privilege change / IRQ from user)
//! - `syscall_kstack` (syscall entry)

use core::ptr::addr_of;

use super::pcb::MAX_PROCESSES;
use crate::gdt::tss;

/// Bytes per process kernel stack (must match nest headroom needs).
pub const KSTACK_SIZE: usize = 16 * 1024;

#[repr(C, align(16))]
struct KStackBytes([u8; KSTACK_SIZE]);

const N_USER_KSTACKS: usize = MAX_PROCESSES - 1;

/// Stacks for process slots `1 .. MAX_PROCESSES` (index 0 → slot 1).
static mut USER_KSTACKS: [KStackBytes; N_USER_KSTACKS] =
    [const { KStackBytes([0; KSTACK_SIZE]) }; N_USER_KSTACKS];

/// Kernel stack top (high address) for process table slot `slot`.
pub fn top_for_slot(slot: usize) -> u64 {
    if slot == 0 || slot >= MAX_PROCESSES {
        return tss::kernel_stack_top();
    }
    unsafe {
        let base = addr_of!(USER_KSTACKS[slot - 1]) as *const u8 as u64;
        base + KSTACK_SIZE as u64
    }
}

/// Install this slot’s kernel stack for ring0 entry (TSS + syscall).
pub fn install_for_slot(slot: usize) {
    let top = top_for_slot(slot);
    tss::set_kernel_stack(top);
    unsafe {
        set_syscall_kstack(top);
    }
}

extern "C" {
    fn set_syscall_kstack(rsp: u64);
}
