//! Kernel export table — `EXPORT_SYMBOL`-like for loadable modules.
//!
//! Modules resolve undefined symbols by name against this table. All
//! exported entry points use a **C ABI** so modules never depend on Rust's
//! crate ABI (which is unstable across compilers).

use crate::console;

/// printk: `void munux_printk(const char *s)` — NUL-terminated C string.
/// NASM modules omit `\\n`; linuxkpi `printk` prints the string as-is.
#[no_mangle]
pub extern "C" fn munux_printk(s: *const u8) {
    crate::linuxkpi::print_cstr(s);
    console::put_char(b'\n');
}

/// Write an unsigned 64-bit value in decimal then newline.
#[no_mangle]
pub extern "C" fn munux_printk_u64(v: u64) {
    console::write_u64(v);
    console::println("");
}

fn cstr_name(p: *const u8) -> Option<&'static str> {
    if p.is_null() {
        return None;
    }
    let mut n = 0usize;
    while n < 12 {
        let b = unsafe { core::ptr::read_volatile(p.add(n)) };
        if b == 0 {
            break;
        }
        n += 1;
    }
    if n == 0 {
        return None;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, n) }).ok()
}

/// `int munux_register_chrdev(const char *name, read, write, release)`
///
/// `read`/`write`: `long (*)(char *buf, unsigned long len)` (bytes or -errno).
/// `release` may be null.
#[no_mangle]
pub extern "C" fn munux_register_chrdev(
    name: *const u8,
    read: Option<extern "C" fn(*mut u8, u64) -> i64>,
    write: Option<extern "C" fn(*mut u8, u64) -> i64>,
    release: Option<extern "C" fn()>,
) -> i32 {
    let Some(n) = cstr_name(name) else {
        return -22; // EINVAL
    };
    let owner = super::loading_slot();
    match crate::fs::vcore::register_mod_chrdev(n, read, write, release, owner) {
        Ok(()) => 0,
        Err(crate::fs::vcore::VfsError::Exist) => -17, // EEXIST
        Err(crate::fs::vcore::VfsError::NoMem) => -12, // ENOMEM
        _ => -22,
    }
}

/// `int munux_unregister_chrdev(const char *name)`
#[no_mangle]
pub extern "C" fn munux_unregister_chrdev(name: *const u8) -> i32 {
    let Some(n) = cstr_name(name) else {
        return -22;
    };
    match crate::fs::vcore::unregister_chrdev(n) {
        Ok(()) => 0,
        Err(crate::fs::vcore::VfsError::NoEnt) => -2,   // ENOENT
        Err(crate::fs::vcore::VfsError::Inval) => -16,  // EBUSY (still open)
        _ => -22,
    }
}

/// Known export names (order matches diagnostic listing).
const EXPORT_NAMES: &[&str] = &[
    "munux_printk",
    "munux_printk_u64",
    "munux_register_chrdev",
    "munux_unregister_chrdev",
    "printk",
    "kmalloc",
    "kzalloc",
    "kfree",
    "memcpy",
    "memmove",
    "memset",
    "strlen",
    "__stack_chk_fail",
    "copy_to_user",
    "copy_from_user",
    "misc_register",
    "misc_deregister",
    "register_chrdev",
    "unregister_chrdev",
    "jiffies",
    "msleep",
    "request_irq",
    "free_irq",
    "complete",
    "wait_for_completion",
    "wait_for_completion_timeout",
    "spin_lock",
    "spin_unlock",
    "__spin_lock_irqsave",
    "__spin_unlock_irqrestore",
    "mutex_lock",
    "mutex_unlock",
    "ioremap",
    "iounmap",
    "dma_alloc_coherent",
    "dma_free_coherent",
    "pci_register_driver",
    "pci_unregister_driver",
    "pci_enable_device",
    "pci_disable_device",
    "pci_read_config_dword",
    "pci_write_config_dword",
    "pci_iomap",
    "pci_iounmap",
    "pci_read_config_byte",
    "pci_read_config_word",
    "pci_find_capability",
    "munux_add_disk",
    "munux_del_disk",
    "munux_register_nic",
    "munux_unregister_nic",
    "munux_netif_rx",
    "munux_icmp_ping",
];

/// Look up an exported symbol by name. Returns absolute address or None.
///
/// Addresses are resolved at call time (function pointers are not valid in
/// `const` contexts on this toolchain).
pub fn lookup(name: &str) -> Option<u64> {
    use crate::linuxkpi;
    match name {
        "munux_printk" => Some(munux_printk as usize as u64),
        "munux_printk_u64" => Some(munux_printk_u64 as usize as u64),
        "munux_register_chrdev" => Some(munux_register_chrdev as usize as u64),
        "munux_unregister_chrdev" => Some(munux_unregister_chrdev as usize as u64),
        "printk" => Some(linuxkpi::printk as usize as u64),
        "kmalloc" => Some(linuxkpi::linux_kmalloc as usize as u64),
        "kzalloc" => Some(linuxkpi::linux_kzalloc as usize as u64),
        "kfree" => Some(linuxkpi::linux_kfree as usize as u64),
        "memcpy" => Some(linuxkpi::linux_memcpy as usize as u64),
        "memmove" => Some(linuxkpi::linux_memmove as usize as u64),
        "memset" => Some(linuxkpi::linux_memset as usize as u64),
        "strlen" => Some(linuxkpi::linux_strlen as usize as u64),
        "__stack_chk_fail" => Some(linuxkpi::__stack_chk_fail as usize as u64),
        "copy_to_user" => Some(linuxkpi::copy_to_user as usize as u64),
        "copy_from_user" => Some(linuxkpi::copy_from_user as usize as u64),
        "misc_register" => Some(linuxkpi::fs::misc_register as usize as u64),
        "misc_deregister" => Some(linuxkpi::fs::misc_deregister as usize as u64),
        "register_chrdev" => Some(linuxkpi::fs::register_chrdev as usize as u64),
        "unregister_chrdev" => Some(linuxkpi::fs::unregister_chrdev as usize as u64),
        "jiffies" => Some(core::ptr::addr_of!(linuxkpi::irq::jiffies) as u64),
        "msleep" => Some(linuxkpi::sync::msleep as usize as u64),
        "request_irq" => Some(linuxkpi::irq::request_irq as usize as u64),
        "free_irq" => Some(linuxkpi::irq::free_irq as usize as u64),
        "complete" => Some(linuxkpi::sync::complete as usize as u64),
        "wait_for_completion" => Some(linuxkpi::sync::wait_for_completion as usize as u64),
        "wait_for_completion_timeout" => {
            Some(linuxkpi::sync::wait_for_completion_timeout as usize as u64)
        }
        "spin_lock" => Some(linuxkpi::sync::spin_lock as usize as u64),
        "spin_unlock" => Some(linuxkpi::sync::spin_unlock as usize as u64),
        "__spin_lock_irqsave" => Some(linuxkpi::sync::__spin_lock_irqsave as usize as u64),
        "__spin_unlock_irqrestore" => {
            Some(linuxkpi::sync::__spin_unlock_irqrestore as usize as u64)
        }
        "mutex_lock" => Some(linuxkpi::sync::mutex_lock as usize as u64),
        "mutex_unlock" => Some(linuxkpi::sync::mutex_unlock as usize as u64),
        "ioremap" => Some(linuxkpi::mmio::ioremap as usize as u64),
        "iounmap" => Some(linuxkpi::mmio::iounmap as usize as u64),
        "dma_alloc_coherent" => Some(linuxkpi::mmio::dma_alloc_coherent as usize as u64),
        "dma_free_coherent" => Some(linuxkpi::mmio::dma_free_coherent as usize as u64),
        "pci_register_driver" => Some(linuxkpi::pci::pci_register_driver as usize as u64),
        "pci_unregister_driver" => Some(linuxkpi::pci::pci_unregister_driver as usize as u64),
        "pci_enable_device" => Some(linuxkpi::pci::pci_enable_device as usize as u64),
        "pci_disable_device" => Some(linuxkpi::pci::pci_disable_device as usize as u64),
        "pci_read_config_dword" => Some(linuxkpi::pci::pci_read_config_dword as usize as u64),
        "pci_write_config_dword" => Some(linuxkpi::pci::pci_write_config_dword as usize as u64),
        "pci_iomap" => Some(linuxkpi::pci::pci_iomap as usize as u64),
        "pci_iounmap" => Some(linuxkpi::pci::pci_iounmap as usize as u64),
        "pci_read_config_byte" => Some(linuxkpi::pci::pci_read_config_byte as usize as u64),
        "pci_read_config_word" => Some(linuxkpi::pci::pci_read_config_word as usize as u64),
        "pci_find_capability" => Some(linuxkpi::pci::pci_find_capability as usize as u64),
        "munux_add_disk" => Some(linuxkpi::blk::munux_add_disk as usize as u64),
        "munux_del_disk" => Some(linuxkpi::blk::munux_del_disk as usize as u64),
        "munux_register_nic" => Some(linuxkpi::net::munux_register_nic as usize as u64),
        "munux_unregister_nic" => Some(linuxkpi::net::munux_unregister_nic as usize as u64),
        "munux_netif_rx" => Some(linuxkpi::net::munux_netif_rx as usize as u64),
        "munux_icmp_ping" => Some(linuxkpi::net::munux_icmp_ping as usize as u64),
        _ => None,
    }
}

/// Number of exported symbols (for diagnostics).
pub fn count() -> usize {
    EXPORT_NAMES.len()
}

/// Name of export at index (for shell listing).
pub fn name_at(i: usize) -> Option<&'static str> {
    EXPORT_NAMES.get(i).copied()
}
