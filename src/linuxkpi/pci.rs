//! PCI config scan + `pci_register_driver`.

use crate::console;
use crate::x86::io::{inl, outl};

const PCI_ADDR: u16 = 0xCF8;
const PCI_DATA: u16 = 0xCFC;
const MAX_DEV: usize = 32;
const MAX_BIND: usize = 32;
const PCI_ANY: u32 = 0xFFFF_FFFF;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PciDev {
    pub bus: u32,
    pub devfn: u32,
    pub vendor: u16,
    pub device: u16,
    pub subsystem_vendor: u16,
    pub subsystem_device: u16,
    pub class: u32,
    pub irq: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PciDeviceId {
    pub vendor: u32,
    pub device: u32,
    pub subvendor: u32,
    pub subdevice: u32,
    pub class: u32,
    pub class_mask: u32,
    pub driver_data: u64,
}

#[repr(C)]
pub struct PciDriver {
    pub name: *const u8,
    pub id_table: *const PciDeviceId,
    pub probe: Option<extern "C" fn(*mut PciDev, *const PciDeviceId) -> i32>,
    pub remove: Option<extern "C" fn(*mut PciDev)>,
}

static mut DEVS: [PciDev; MAX_DEV] = [PciDev {
    bus: 0,
    devfn: 0,
    vendor: 0,
    device: 0,
    subsystem_vendor: 0,
    subsystem_device: 0,
    class: 0,
    irq: 0,
}; MAX_DEV];
static mut NDEV: usize = 0;

#[derive(Clone, Copy)]
struct Binding {
    used: bool,
    dev_i: usize,
    drv: *mut PciDriver,
}
static mut BINDS: [Binding; MAX_BIND] = [Binding {
    used: false,
    dev_i: 0,
    drv: core::ptr::null_mut(),
}; MAX_BIND];

fn devs_mut() -> &'static mut [PciDev; MAX_DEV] {
    unsafe { &mut *core::ptr::addr_of_mut!(DEVS) }
}

fn ndev() -> usize {
    unsafe { core::ptr::read(core::ptr::addr_of!(NDEV)) }
}

fn set_ndev(n: usize) {
    unsafe {
        core::ptr::write(core::ptr::addr_of_mut!(NDEV), n);
    }
}

fn binds_mut() -> &'static mut [Binding; MAX_BIND] {
    unsafe { &mut *core::ptr::addr_of_mut!(BINDS) }
}

fn cfg_addr(bus: u32, slot: u32, func: u32, offset: u32) -> u32 {
    0x8000_0000 | (bus << 16) | (slot << 11) | (func << 8) | (offset & 0xFC)
}

fn read_cfg(bus: u32, slot: u32, func: u32, offset: u32) -> u32 {
    unsafe {
        outl(PCI_ADDR, cfg_addr(bus, slot, func, offset));
        inl(PCI_DATA)
    }
}

fn write_cfg(bus: u32, slot: u32, func: u32, offset: u32, val: u32) {
    unsafe {
        outl(PCI_ADDR, cfg_addr(bus, slot, func, offset));
        outl(PCI_DATA, val);
    }
}

fn push_dev(d: PciDev) {
    let n = ndev();
    if n >= MAX_DEV {
        return;
    }
    devs_mut()[n] = d;
    set_ndev(n + 1);
}

fn scan_func(bus: u32, slot: u32, func: u32) {
    let id = read_cfg(bus, slot, func, 0);
    let vendor = (id & 0xFFFF) as u16;
    if vendor == 0xFFFF {
        return;
    }
    let device = ((id >> 16) & 0xFFFF) as u16;
    let classw = read_cfg(bus, slot, func, 0x08);
    let class = classw >> 8;
    let subsys = read_cfg(bus, slot, func, 0x2C);
    let irq_reg = read_cfg(bus, slot, func, 0x3C);
    push_dev(PciDev {
        bus,
        devfn: (slot << 3) | func,
        vendor,
        device,
        subsystem_vendor: (subsys & 0xFFFF) as u16,
        subsystem_device: ((subsys >> 16) & 0xFFFF) as u16,
        class,
        irq: irq_reg & 0xFF,
    });
}

/// Scan bus 0 (QEMU i440FX). Call once after PIC init.
pub fn init() {
    set_ndev(0);
    for slot in 0..32u32 {
        let id = read_cfg(0, slot, 0, 0);
        if (id & 0xFFFF) == 0xFFFF {
            continue;
        }
        let ht = (read_cfg(0, slot, 0, 0x0C) >> 16) as u8;
        let nfunc = if ht & 0x80 != 0 { 8 } else { 1 };
        for func in 0..nfunc {
            scan_func(0, slot, func);
        }
    }
    console::print("pci: devices=");
    console::write_u64(ndev() as u64);
    console::println("");
    let n = ndev();
    let dtab = devs_mut();
    for i in 0..n {
        let d = dtab[i];
        console::print("  ");
        console::write_hex64(d.vendor as u64);
        console::print(":");
        console::write_hex64(d.device as u64);
        console::println("");
    }
}

fn id_end(id: &PciDeviceId) -> bool {
    id.vendor == 0 && id.device == 0 && id.class == 0 && id.class_mask == 0
}

fn id_match(id: &PciDeviceId, d: &PciDev) -> bool {
    if id.vendor != PCI_ANY && id.vendor != d.vendor as u32 {
        return false;
    }
    if id.device != PCI_ANY && id.device != d.device as u32 {
        return false;
    }
    if id.subvendor != 0 && id.subvendor != PCI_ANY && id.subvendor != d.subsystem_vendor as u32 {
        return false;
    }
    if id.subdevice != 0 && id.subdevice != PCI_ANY && id.subdevice != d.subsystem_device as u32 {
        return false;
    }
    if id.class_mask != 0 && (d.class & id.class_mask) != id.class {
        return false;
    }
    true
}

fn already_bound(dev_i: usize) -> bool {
    binds_mut().iter().any(|b| b.used && b.dev_i == dev_i)
}

fn bind(dev_i: usize, drv: *mut PciDriver) -> bool {
    for b in binds_mut().iter_mut() {
        if !b.used {
            b.used = true;
            b.dev_i = dev_i;
            b.drv = drv;
            return true;
        }
    }
    false
}

/// Linux returns 0 on success (even with zero matches). We still call every probe.
pub extern "C" fn pci_register_driver(drv: *mut PciDriver) -> i32 {
    if drv.is_null() {
        return -22;
    }
    let table = unsafe { (*drv).id_table };
    if table.is_null() {
        return -22;
    }
    let probe = unsafe { (*drv).probe };
    let mut i = 0usize;
    loop {
        let id = unsafe { &*table.add(i) };
        if id_end(id) {
            break;
        }
        let n = ndev();
        let dtab = devs_mut();
        for di in 0..n {
            if already_bound(di) {
                continue;
            }
            if !id_match(id, &dtab[di]) {
                continue;
            }
            if let Some(p) = probe {
                let rc = p(core::ptr::addr_of_mut!(dtab[di]), id);
                if rc == 0 {
                    let _ = bind(di, drv);
                }
            }
        }
        i += 1;
        if i > 64 {
            break;
        }
    }
    0
}

pub extern "C" fn pci_unregister_driver(drv: *mut PciDriver) {
    if drv.is_null() {
        return;
    }
    let remove = unsafe { (*drv).remove };
    let dtab = devs_mut();
    for b in binds_mut().iter_mut() {
        if b.used && b.drv == drv {
            if let Some(r) = remove {
                r(core::ptr::addr_of_mut!(dtab[b.dev_i]));
            }
            b.used = false;
            b.drv = core::ptr::null_mut();
        }
    }
}

fn split_devfn(dev: &PciDev) -> (u32, u32, u32) {
    let slot = (dev.devfn >> 3) & 0x1F;
    let func = dev.devfn & 0x7;
    (dev.bus, slot, func)
}

pub extern "C" fn pci_enable_device(dev: *mut PciDev) -> i32 {
    if dev.is_null() {
        return -22;
    }
    let d = unsafe { &*dev };
    let (bus, slot, func) = split_devfn(d);
    let cmd = read_cfg(bus, slot, func, 0x04);
    write_cfg(bus, slot, func, 0x04, cmd | 0x7); // IO+MEM+master
    0
}

pub extern "C" fn pci_disable_device(dev: *mut PciDev) {
    if dev.is_null() {
        return;
    }
    let d = unsafe { &*dev };
    let (bus, slot, func) = split_devfn(d);
    let cmd = read_cfg(bus, slot, func, 0x04);
    write_cfg(bus, slot, func, 0x04, cmd & !0x7);
}

pub extern "C" fn pci_read_config_dword(dev: *mut PciDev, where_: i32, val: *mut u32) -> i32 {
    if dev.is_null() || val.is_null() || where_ < 0 {
        return -22;
    }
    let d = unsafe { &*dev };
    let (bus, slot, func) = split_devfn(d);
    unsafe {
        *val = read_cfg(bus, slot, func, where_ as u32);
    }
    0
}

pub extern "C" fn pci_read_config_byte(dev: *mut PciDev, where_: i32, val: *mut u8) -> i32 {
    if val.is_null() || where_ < 0 {
        return -22;
    }
    let mut dw = 0u32;
    let rc = pci_read_config_dword(dev, where_ & !3, core::ptr::addr_of_mut!(dw));
    if rc != 0 {
        return rc;
    }
    let shift = ((where_ & 3) * 8) as u32;
    unsafe {
        *val = ((dw >> shift) & 0xff) as u8;
    }
    0
}

pub extern "C" fn pci_read_config_word(dev: *mut PciDev, where_: i32, val: *mut u16) -> i32 {
    if val.is_null() || where_ < 0 {
        return -22;
    }
    let mut dw = 0u32;
    let rc = pci_read_config_dword(dev, where_ & !3, core::ptr::addr_of_mut!(dw));
    if rc != 0 {
        return rc;
    }
    let shift = ((where_ & 2) * 8) as u32;
    unsafe {
        *val = ((dw >> shift) & 0xffff) as u16;
    }
    0
}

/// First capability with `id`, or 0.
pub extern "C" fn pci_find_capability(dev: *mut PciDev, id: i32) -> u8 {
    let id = id as u8;
    let mut status = 0u16;
    if pci_read_config_word(dev, 0x06, core::ptr::addr_of_mut!(status)) != 0 {
        return 0;
    }
    if status & (1 << 4) == 0 {
        return 0; // no cap list
    }
    let mut pos = 0u8;
    if pci_read_config_byte(dev, 0x34, core::ptr::addr_of_mut!(pos)) != 0 {
        return 0;
    }
    pos &= 0xFC;
    for _ in 0..48 {
        if pos < 0x40 {
            break;
        }
        let mut cid = 0u8;
        if pci_read_config_byte(dev, pos as i32, core::ptr::addr_of_mut!(cid)) != 0 {
            break;
        }
        if cid == id {
            return pos;
        }
        if pci_read_config_byte(dev, pos as i32 + 1, core::ptr::addr_of_mut!(pos)) != 0 {
            break;
        }
        pos &= 0xFC;
    }
    0
}

pub extern "C" fn pci_write_config_dword(dev: *mut PciDev, where_: i32, val: u32) -> i32 {
    if dev.is_null() || where_ < 0 {
        return -22;
    }
    let d = unsafe { &*dev };
    let (bus, slot, func) = split_devfn(d);
    write_cfg(bus, slot, func, where_ as u32, val);
    0
}

pub extern "C" fn pci_iomap(dev: *mut PciDev, bar: i32, max: u64) -> *mut u8 {
    if dev.is_null() || !(0..6).contains(&bar) {
        return core::ptr::null_mut();
    }
    let d = unsafe { &*dev };
    let (bus, slot, func) = split_devfn(d);
    let off = 0x10 + (bar as u32) * 4;
    let raw = read_cfg(bus, slot, func, off);
    if raw == 0 || raw == 0xFFFF_FFFF {
        return core::ptr::null_mut();
    }
    if raw & 1 != 0 {
        return core::ptr::null_mut(); // I/O BAR
    }
    let mut phys = (raw & 0xFFFF_FFF0) as u64;
    if (raw & 0x6) == 0x4 {
        let hi = read_cfg(bus, slot, func, off + 4) as u64;
        phys |= hi << 32;
    }
    let size = if max == 0 { 0x1000 } else { max };
    super::mmio::ioremap(phys, size)
}

pub extern "C" fn pci_iounmap(_dev: *mut PciDev, addr: *mut u8) {
    super::mmio::iounmap(addr);
}
