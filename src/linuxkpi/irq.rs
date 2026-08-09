//! `request_irq` / `free_irq` + `jiffies`.

use crate::interrupts::idt::register_interrupt_handler;
use crate::interrupts::pic;
use crate::module;

const IRQF_SHARED: u64 = 0x80;
const MAX_IRQ: usize = 16;
const SLOTS: usize = 2;

pub type IrqHandler = extern "C" fn(i32, *mut u8) -> i32;

#[derive(Clone, Copy)]
struct IrqAction {
    used: bool,
    shared: bool,
    handler: Option<IrqHandler>,
    dev: *mut u8,
}

impl IrqAction {
    const fn empty() -> Self {
        Self {
            used: false,
            shared: false,
            handler: None,
            dev: core::ptr::null_mut(),
        }
    }
}

static mut ACTIONS: [[IrqAction; SLOTS]; MAX_IRQ] = [[IrqAction::empty(); SLOTS]; MAX_IRQ];

/// Linux `jiffies` storage (unsigned long). Exported by address.
#[no_mangle]
pub static mut jiffies: u64 = 0;

pub fn set_jiffies(v: u64) {
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(jiffies), v);
    }
}

fn actions() -> &'static mut [[IrqAction; SLOTS]; MAX_IRQ] {
    unsafe { &mut *core::ptr::addr_of_mut!(ACTIONS) }
}

extern "C" {
    fn isr_irq2();
    fn isr_irq3();
    fn isr_irq4();
    fn isr_irq5();
    fn isr_irq6();
    fn isr_irq7();
    fn isr_irq8();
    fn isr_irq9();
    fn isr_irq10();
    fn isr_irq11();
    fn isr_irq12();
    fn isr_irq13();
    fn isr_irq14();
    fn isr_irq15();
}

fn stub_for(irq: u8) -> Option<unsafe extern "C" fn()> {
    match irq {
        2 => Some(isr_irq2),
        3 => Some(isr_irq3),
        4 => Some(isr_irq4),
        5 => Some(isr_irq5),
        6 => Some(isr_irq6),
        7 => Some(isr_irq7),
        8 => Some(isr_irq8),
        9 => Some(isr_irq9),
        10 => Some(isr_irq10),
        11 => Some(isr_irq11),
        12 => Some(isr_irq12),
        13 => Some(isr_irq13),
        14 => Some(isr_irq14),
        15 => Some(isr_irq15),
        _ => None,
    }
}

fn irq_occupied_exclusive(irq: usize) -> bool {
    actions()[irq].iter().any(|a| a.used && !a.shared)
}

fn irq_any(irq: usize) -> bool {
    actions()[irq].iter().any(|a| a.used)
}

/// Called from PIT / PIC stubs. IRQ0/1 are chained; 2–15 take full IDT.
pub fn dispatch(irq: u32) {
    if irq as usize >= MAX_IRQ {
        return;
    }
    module::map_code_into_current();
    let table = actions()[irq as usize];
    for a in table.iter() {
        if !a.used {
            continue;
        }
        if let Some(h) = a.handler {
            let _ = h(irq as i32, a.dev);
        }
    }
}

/// IDT entry for IRQ2–15 (EOI included).
#[no_mangle]
pub extern "C" fn linux_irq_dispatch(irq: u32) {
    dispatch(irq);
    unsafe {
        if irq >= 8 {
            pic::eoi_slave();
        } else {
            pic::eoi_master();
        }
    }
}

pub extern "C" fn request_irq(
    irq: u32,
    handler: Option<IrqHandler>,
    flags: u64,
    _name: *const u8,
    dev: *mut u8,
) -> i32 {
    if irq as usize >= MAX_IRQ || handler.is_none() {
        return -22; // EINVAL
    }
    let shared = (flags & IRQF_SHARED) != 0;
    let i = irq as usize;
    if irq_occupied_exclusive(i) && !shared {
        return -16; // EBUSY
    }
    if irq_any(i) && !shared {
        return -16;
    }
    if irq_any(i) {
        let existing_shared = actions()[i].iter().filter(|a| a.used).all(|a| a.shared);
        if !existing_shared || !shared {
            return -16;
        }
    }

    let slot = actions()[i].iter_mut().find(|a| !a.used);
    let Some(slot) = slot else {
        return -12; // ENOMEM
    };
    slot.used = true;
    slot.shared = shared;
    slot.handler = handler;
    slot.dev = dev;

    if irq >= 2 {
        if let Some(stub) = stub_for(irq as u8) {
            register_interrupt_handler(32 + irq as u8, stub);
        }
        pic::unmask_irq(irq as u8);
    }
    0
}

pub extern "C" fn free_irq(irq: u32, dev: *mut u8) {
    if irq as usize >= MAX_IRQ {
        return;
    }
    let i = irq as usize;
    for a in actions()[i].iter_mut() {
        if a.used && a.dev == dev {
            *a = IrqAction::empty();
            break;
        }
    }
    if irq >= 2 && !irq_any(i) {
        pic::mask_irq(irq as u8);
    }
}
