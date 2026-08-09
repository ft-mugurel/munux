//! Linux kernel C API shims (linuxkpi L1–L4).
//!
//! Modules resolve these by **name** via `module::export` (not the Rust crate ABI).
//! Keep signatures C-compatible. Do not `#[no_mangle]` names that collide with
//! `compiler_builtins` (`memcpy` / `memset`) — the export table holds the address.

pub mod blk;
pub mod fs;
pub mod irq;
pub mod mmio;
pub mod pci;
pub mod sync;
pub mod uaccess;

use crate::console;
use crate::memory;

pub use fs::{
    call_open, call_read, call_release, call_write, LinuxFileOperations, MiscDevice,
};
pub use uaccess::{copy_from_user, copy_to_user};

/// Print a NUL-terminated C string. Skips Linux `KERN_SOH` + level if present.
pub fn print_cstr(s: *const u8) {
    if s.is_null() {
        return;
    }
    let mut n = 0usize;
    while n < 512 {
        let b = unsafe { core::ptr::read_volatile(s.add(n)) };
        if b == 0 {
            break;
        }
        n += 1;
    }
    if n == 0 {
        return;
    }
    let mut i = 0usize;
    // KERN_SOH (0x01) + level digit / 'c'
    if n >= 2 && unsafe { *s } == 1 {
        i = 2;
    }
    while i < n {
        let b = unsafe { *s.add(i) };
        console::put_char(b);
        i += 1;
    }
}

/// `int printk(const char *fmt)` — L1: no varargs; extra C args are unused.
#[no_mangle]
pub extern "C" fn printk(fmt: *const u8) -> i32 {
    print_cstr(fmt);
    0
}

/// Linux `void *kmalloc(size_t, gfp_t)` — flags ignored; heap already zeroes.
#[no_mangle]
pub extern "C" fn linux_kmalloc(size: u64, _flags: u32) -> *mut u8 {
    if size == 0 || size > isize::MAX as u64 {
        return core::ptr::null_mut();
    }
    memory::kmalloc(size as usize).unwrap_or(core::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn linux_kzalloc(size: u64, flags: u32) -> *mut u8 {
    linux_kmalloc(size, flags)
}

#[no_mangle]
pub extern "C" fn linux_kfree(ptr: *mut u8) {
    if !ptr.is_null() {
        memory::kfree(ptr);
    }
}

pub unsafe extern "C" fn linux_memcpy(dst: *mut u8, src: *const u8, n: u64) -> *mut u8 {
    if !dst.is_null() && !src.is_null() && n > 0 {
        core::ptr::copy_nonoverlapping(src, dst, n as usize);
    }
    dst
}

pub unsafe extern "C" fn linux_memmove(dst: *mut u8, src: *const u8, n: u64) -> *mut u8 {
    if !dst.is_null() && !src.is_null() && n > 0 {
        core::ptr::copy(src, dst, n as usize);
    }
    dst
}

pub unsafe extern "C" fn linux_memset(dst: *mut u8, c: i32, n: u64) -> *mut u8 {
    if !dst.is_null() && n > 0 {
        core::ptr::write_bytes(dst, c as u8, n as usize);
    }
    dst
}

pub unsafe extern "C" fn linux_strlen(s: *const u8) -> u64 {
    if s.is_null() {
        return 0;
    }
    let mut n = 0u64;
    while n < 4096 {
        if core::ptr::read_volatile(s.add(n as usize)) == 0 {
            break;
        }
        n += 1;
    }
    n
}

/// Linker/stack-protector stub if a module is built without `-fno-stack-protector`.
pub extern "C" fn __stack_chk_fail() -> ! {
    console::println("linuxkpi: __stack_chk_fail");
    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}
