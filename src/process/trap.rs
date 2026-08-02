//! Interrupt / trap frame for user context save/restore (IRQ preemption).
//!
//! Layout must match `multiboot/irq.asm` after the general-register pushes
//! and before `iretq` (hardware frame for a ring-3 interrupt):
//!
//! ```text
//! [rsp+0]   rax
//! [rsp+8]   rbx
//! ...
//! [rsp+112] r15
//! [rsp+120] rip
//! [rsp+128] cs
//! [rsp+136] rflags
//! [rsp+144] rsp   (user)
//! [rsp+152] ss
//! ```

use crate::gdt::{USER_CODE_SELECTOR, USER_DATA_SELECTOR};

/// Full user-visible state for `iretq` resume after a timer preempt.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct TrapFrame {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl TrapFrame {
    pub const fn zero() -> Self {
        Self {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            cs: 0,
            rflags: 0x202,
            rsp: 0,
            ss: 0,
        }
    }

    /// Build a minimal frame for a Ready task that has never been IRQ-preempted
    /// (e.g. right after `fork`).
    pub fn from_user_entry(rip: u64, rsp: u64, rflags: u64, rax: u64) -> Self {
        let mut t = Self::zero();
        t.rax = rax;
        t.rip = rip;
        t.rsp = rsp;
        t.rflags = rflags | 0x200; // IF
        t.cs = USER_CODE_SELECTOR as u64;
        t.ss = USER_DATA_SELECTOR as u64;
        t
    }

    #[inline]
    pub fn is_user(&self) -> bool {
        (self.cs & 3) == 3
    }
}
