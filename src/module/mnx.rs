//! Munux module container format **MNX1** (not mainline Linux `.ko`).
//!
//! Layout (little-endian):
//! ```text
//! struct MnxHeader {          // 48 bytes
//!     u32 magic;              // 'MNX1' = 0x3158_4E4D
//!     u8  name[28];           // NUL-padded module name
//!     u32 code_len;           // bytes of code image
//!     u32 init_off;           // offset of init() in code
//!     u32 exit_off;           // offset of exit(); 0xFFFF_FFFF = none
//!     u32 n_relocs;
//! };
//! u8  code[code_len];
//! // then n_relocs of:
//! struct MnxReloc {
//!     u32 offset;             // patch site in code (absolute u64 write)
//!     u8  name[32];           // export name, NUL-padded
//! };
//! ```
//!
//! `init` / `exit` are `extern "C" fn() -> i32` (0 = success).

use super::export;
use crate::memory::{kfree, kmalloc};

pub const MNX_MAGIC: u32 = 0x3158_4E4D; // 'MNX1' LE
pub const MNX_HEADER_SIZE: usize = 48;
pub const MNX_NAME_LEN: usize = 28;
pub const MNX_RELOC_SIZE: usize = 36; // 4 + 32
pub const MNX_SYM_LEN: usize = 32;
pub const MNX_EXIT_NONE: u32 = 0xFFFF_FFFF;
pub const MNX_MAX_FILE: usize = 64 * 1024;
pub const MNX_MAX_CODE: usize = 32 * 1024;
pub const MNX_MAX_RELOCS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MnxError {
    BadMagic,
    Truncated,
    BadName,
    BadCode,
    BadReloc,
    Unresolved,
    Oom,
    InitFail,
}

impl MnxError {
    pub fn as_str(self) -> &'static str {
        match self {
            MnxError::BadMagic => "bad magic",
            MnxError::Truncated => "truncated",
            MnxError::BadName => "bad name",
            MnxError::BadCode => "bad code",
            MnxError::BadReloc => "bad reloc",
            MnxError::Unresolved => "unresolved symbol",
            MnxError::Oom => "out of memory",
            MnxError::InitFail => "init failed",
        }
    }
}

/// Parsed + loaded module image (code still owned by caller after success).
pub struct LoadedMnx {
    pub name: [u8; MNX_NAME_LEN],
    pub name_len: usize,
    pub code: *mut u8,
    pub code_len: usize,
    pub init: Option<extern "C" fn() -> i32>,
    pub exit: Option<extern "C" fn() -> i32>,
}

impl LoadedMnx {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }
}

fn read_u32(buf: &[u8], off: usize) -> Result<u32, MnxError> {
    if off + 4 > buf.len() {
        return Err(MnxError::Truncated);
    }
    Ok(u32::from_le_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
    ]))
}

fn cstr_len(bytes: &[u8]) -> usize {
    bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())
}

/// Load an MNX1 blob from memory: allocate code, apply relocs, return hooks.
pub fn load_from_bytes(buf: &[u8]) -> Result<LoadedMnx, MnxError> {
    if buf.len() < MNX_HEADER_SIZE {
        return Err(MnxError::Truncated);
    }
    let magic = read_u32(buf, 0)?;
    if magic != MNX_MAGIC {
        return Err(MnxError::BadMagic);
    }

    let mut name = [0u8; MNX_NAME_LEN];
    name.copy_from_slice(&buf[4..4 + MNX_NAME_LEN]);
    let name_len = cstr_len(&name);
    if name_len == 0 || name_len > MNX_NAME_LEN {
        return Err(MnxError::BadName);
    }
    // Reject non-printable names.
    for &b in &name[..name_len] {
        if b < 0x20 || b > 0x7e {
            return Err(MnxError::BadName);
        }
    }

    let code_len = read_u32(buf, 4 + MNX_NAME_LEN)? as usize;
    let init_off = read_u32(buf, 4 + MNX_NAME_LEN + 4)?;
    let exit_off = read_u32(buf, 4 + MNX_NAME_LEN + 8)?;
    let n_relocs = read_u32(buf, 4 + MNX_NAME_LEN + 12)? as usize;

    if code_len == 0 || code_len > MNX_MAX_CODE {
        return Err(MnxError::BadCode);
    }
    if n_relocs > MNX_MAX_RELOCS {
        return Err(MnxError::BadReloc);
    }
    if (init_off as usize) >= code_len {
        return Err(MnxError::BadCode);
    }
    if exit_off != MNX_EXIT_NONE && (exit_off as usize) >= code_len {
        return Err(MnxError::BadCode);
    }

    let code_start = MNX_HEADER_SIZE;
    let code_end = code_start + code_len;
    let reloc_end = code_end + n_relocs * MNX_RELOC_SIZE;
    if reloc_end > buf.len() {
        return Err(MnxError::Truncated);
    }

    let code = kmalloc(code_len).ok_or(MnxError::Oom)?;
    unsafe {
        core::ptr::copy_nonoverlapping(buf[code_start..code_end].as_ptr(), code, code_len);
    }

    // Apply absolute 64-bit relocations (write export address at offset).
    for i in 0..n_relocs {
        let ro = code_end + i * MNX_RELOC_SIZE;
        let off = match read_u32(buf, ro) {
            Ok(v) => v as usize,
            Err(e) => {
                kfree(code);
                return Err(e);
            }
        };
        if off + 8 > code_len {
            kfree(code);
            return Err(MnxError::BadReloc);
        }
        let sym_bytes = &buf[ro + 4..ro + 4 + MNX_SYM_LEN];
        let slen = cstr_len(sym_bytes);
        let sym = match core::str::from_utf8(&sym_bytes[..slen]) {
            Ok(s) if !s.is_empty() => s,
            _ => {
                kfree(code);
                return Err(MnxError::BadReloc);
            }
        };
        let addr = match export::lookup(sym) {
            Some(a) => a,
            None => {
                kfree(code);
                return Err(MnxError::Unresolved);
            }
        };
        unsafe {
            let p = code.add(off) as *mut u64;
            core::ptr::write_unaligned(p, addr);
        }
    }

    let init: Option<extern "C" fn() -> i32> = Some(unsafe {
        core::mem::transmute(code.add(init_off as usize) as usize)
    });
    let exit: Option<extern "C" fn() -> i32> = if exit_off == MNX_EXIT_NONE {
        None
    } else {
        Some(unsafe { core::mem::transmute(code.add(exit_off as usize) as usize) })
    };

    Ok(LoadedMnx {
        name,
        name_len,
        code,
        code_len,
        init,
        exit,
    })
}

/// Free a code image previously returned by [`load_from_bytes`].
pub unsafe fn free_code(code: *mut u8) {
    if !code.is_null() {
        kfree(code);
    }
}
