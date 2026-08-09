//! `copy_to_user` / `copy_from_user`.
//!
//! munux VFS stages syscall buffers in kernel memory before fops run, so these
//! are memcpy for L2. Return value matches Linux: 0 = ok, >0 = bytes not copied.

pub unsafe extern "C" fn copy_to_user(to: *mut u8, from: *const u8, n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    if to.is_null() || from.is_null() {
        return n;
    }
    core::ptr::copy_nonoverlapping(from, to, n as usize);
    0
}

pub unsafe extern "C" fn copy_from_user(to: *mut u8, from: *const u8, n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    if to.is_null() || from.is_null() {
        return n;
    }
    core::ptr::copy_nonoverlapping(from, to, n as usize);
    0
}
