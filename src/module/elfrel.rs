//! ELF64 **ET_REL** loader (munux `.ko` — conceptual LKM, not mainline vermagic).
//!
//! Layout: allocatable sections packed into one `kmalloc` image, then
//! `R_X86_64_{64,PC32,PLT32,32,32S}` applied. Undefined `PC32`/`PLT32`
//! (typical `call munux_*`) cannot reach kernel text from the high heap, so
//! the loader emits a tiny abs64 trampoline per unique export:
//!
//! ```text
//!   mov rax, <export>
//!   jmp rax
//! ```
//!
//! Init/exit: global `init_module` / `cleanup_module` (Linux classic names).
//! Module name: `.modinfo` `name=…` if present, else caller hint.

use super::export;
use super::mnx::{LoadedMnx, MnxError, MNX_MAX_CODE, MNX_MAX_FILE, MNX_NAME_LEN};
use crate::memory::{kfree, kmalloc};

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_REL: u16 = 1;
const EM_X86_64: u16 = 62;

const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;

const SHF_ALLOC: u64 = 0x2;

const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xfff1;
const SHN_COMMON: u16 = 0xfff2;

const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_32: u32 = 10;
const R_X86_64_32S: u32 = 11;

const MAX_SECTIONS: usize = 32;
const MAX_TRAMP: usize = 16;
const TRAMP_STRIDE: usize = 16; // 12-byte stub + pad
const EHSIZE: usize = 64;
const SHSIZE: usize = 64;
const SYMSIZE: usize = 24;
const RELASIZE: usize = 24;

pub fn is_elf(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[..4] == ELFMAG
}

fn read_u16(buf: &[u8], off: usize) -> Result<u16, MnxError> {
    if off + 2 > buf.len() {
        return Err(MnxError::Truncated);
    }
    Ok(u16::from_le_bytes([buf[off], buf[off + 1]]))
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

fn read_u64(buf: &[u8], off: usize) -> Result<u64, MnxError> {
    if off + 8 > buf.len() {
        return Err(MnxError::Truncated);
    }
    Ok(u64::from_le_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
        buf[off + 4],
        buf[off + 5],
        buf[off + 6],
        buf[off + 7],
    ]))
}

fn read_i64(buf: &[u8], off: usize) -> Result<i64, MnxError> {
    Ok(read_u64(buf, off)? as i64)
}

fn align_up(v: u64, a: u64) -> u64 {
    if a <= 1 {
        return v;
    }
    (v + a - 1) & !(a - 1)
}

struct Shdr {
    name: u32,
    typ: u32,
    flags: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    addralign: u64,
    entsize: u64,
}

fn parse_shdr(buf: &[u8], off: usize) -> Result<Shdr, MnxError> {
    Ok(Shdr {
        name: read_u32(buf, off)?,
        typ: read_u32(buf, off + 4)?,
        flags: read_u64(buf, off + 8)?,
        offset: read_u64(buf, off + 24)?,
        size: read_u64(buf, off + 32)?,
        link: read_u32(buf, off + 40)?,
        info: read_u32(buf, off + 44)?,
        addralign: read_u64(buf, off + 48)?,
        entsize: read_u64(buf, off + 56)?,
    })
}

fn sh_name<'a>(buf: &'a [u8], shstr: &Shdr, name_off: u32) -> Result<&'a str, MnxError> {
    let start = shstr.offset as usize + name_off as usize;
    if start >= buf.len() {
        return Err(MnxError::Truncated);
    }
    let max = (shstr.offset as usize + shstr.size as usize).min(buf.len());
    if start >= max {
        return Err(MnxError::Truncated);
    }
    let slice = &buf[start..max];
    let n = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    core::str::from_utf8(&slice[..n]).map_err(|_| MnxError::BadName)
}

fn parse_modinfo_name(bytes: &[u8]) -> Option<&str> {
    let mut i = 0usize;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 5] == b"name=" {
            let start = i + 5;
            let mut end = start;
            while end < bytes.len() && bytes[end] != 0 && bytes[end] != b'\n' {
                end += 1;
            }
            if end > start {
                return core::str::from_utf8(&bytes[start..end]).ok();
            }
            return None;
        }
        i += 1;
    }
    None
}

struct Sym {
    name: u32,
    shndx: u16,
    value: u64,
}

fn parse_sym(buf: &[u8], off: usize) -> Result<Sym, MnxError> {
    Ok(Sym {
        name: read_u32(buf, off)?,
        shndx: read_u16(buf, off + 6)?,
        value: read_u64(buf, off + 8)?,
    })
}

fn strtab_get<'a>(buf: &'a [u8], strtab: &Shdr, off: u32) -> Result<&'a str, MnxError> {
    let start = strtab.offset as usize + off as usize;
    let max = (strtab.offset as usize + strtab.size as usize).min(buf.len());
    if start >= max {
        return Ok("");
    }
    let slice = &buf[start..max];
    let n = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    core::str::from_utf8(&slice[..n]).map_err(|_| MnxError::BadName)
}

#[derive(Clone, Copy)]
struct Tramp {
    used: bool,
    name: [u8; 32],
    nlen: u8,
    addr: u64,
}

fn tramp_matches(t: &Tramp, name: &str) -> bool {
    t.used && t.nlen as usize == name.len() && &t.name[..t.nlen as usize] == name.as_bytes()
}

fn emit_tramp(tramp_va: u64, dest: u64) {
    // mov rax, imm64 ; jmp rax
    let mut stub = [0u8; 12];
    stub[0] = 0x48;
    stub[1] = 0xB8;
    stub[2..10].copy_from_slice(&dest.to_le_bytes());
    stub[10] = 0xFF;
    stub[11] = 0xE0;
    unsafe {
        core::ptr::copy_nonoverlapping(stub.as_ptr(), tramp_va as *mut u8, 12);
    }
}

/// Load ELF64 ET_REL. `name_hint` used when `.modinfo` has no `name=`.
pub fn load_from_bytes(buf: &[u8], name_hint: &str) -> Result<LoadedMnx, MnxError> {
    if buf.len() > MNX_MAX_FILE || buf.len() < EHSIZE {
        return Err(MnxError::Truncated);
    }
    if buf[..4] != ELFMAG {
        return Err(MnxError::BadMagic);
    }
    if buf[4] != ELFCLASS64 || buf[5] != ELFDATA2LSB || buf[6] != EV_CURRENT {
        return Err(MnxError::BadMagic);
    }
    let e_type = read_u16(buf, 16)?;
    let e_machine = read_u16(buf, 18)?;
    if e_type != ET_REL || e_machine != EM_X86_64 {
        return Err(MnxError::BadMagic);
    }
    let e_shoff = read_u64(buf, 40)? as usize;
    let e_shentsize = read_u16(buf, 58)? as usize;
    let e_shnum = read_u16(buf, 60)? as usize;
    let e_shstrndx = read_u16(buf, 62)? as usize;
    if e_shentsize != SHSIZE || e_shnum == 0 || e_shnum > MAX_SECTIONS {
        return Err(MnxError::BadCode);
    }
    if e_shstrndx >= e_shnum {
        return Err(MnxError::BadCode);
    }
    if e_shoff.saturating_add(e_shnum * SHSIZE) > buf.len() {
        return Err(MnxError::Truncated);
    }

    let mut shdrs: [Option<Shdr>; MAX_SECTIONS] = [(); MAX_SECTIONS].map(|_| None);
    for i in 0..e_shnum {
        shdrs[i] = Some(parse_shdr(buf, e_shoff + i * SHSIZE)?);
    }
    let shstr = shdrs[e_shstrndx].as_ref().ok_or(MnxError::BadCode)?;

    // Optional module name from .modinfo (file bytes, not necessarily loaded).
    let mut parsed_name = [0u8; MNX_NAME_LEN];
    let mut parsed_nlen = 0usize;
    for i in 1..e_shnum {
        let sh = shdrs[i].as_ref().unwrap();
        if sh.typ != SHT_PROGBITS {
            continue;
        }
        let nm = sh_name(buf, shstr, sh.name)?;
        if nm != ".modinfo" {
            continue;
        }
        let start = sh.offset as usize;
        let end = start.saturating_add(sh.size as usize);
        if end > buf.len() {
            return Err(MnxError::Truncated);
        }
        if let Some(n) = parse_modinfo_name(&buf[start..end]) {
            let b = n.as_bytes();
            let k = b.len().min(MNX_NAME_LEN);
            parsed_name[..k].copy_from_slice(&b[..k]);
            parsed_nlen = k;
        }
        break;
    }

    // Layout SHF_ALLOC sections.
    let mut sec_base = [0u64; MAX_SECTIONS];
    let mut layout = 0u64;
    for i in 1..e_shnum {
        let sh = shdrs[i].as_ref().unwrap();
        if sh.flags & SHF_ALLOC == 0 {
            continue;
        }
        if sh.typ != SHT_PROGBITS && sh.typ != SHT_NOBITS {
            continue;
        }
        let al = if sh.addralign == 0 { 1 } else { sh.addralign };
        layout = align_up(layout, al);
        sec_base[i] = layout;
        layout = layout.saturating_add(sh.size);
    }
    let tramp_off = align_up(layout, 16);
    let total = tramp_off as usize + MAX_TRAMP * TRAMP_STRIDE;
    if total == 0 || total > MNX_MAX_CODE + MAX_TRAMP * TRAMP_STRIDE {
        return Err(MnxError::BadCode);
    }

    let code = kmalloc(total).ok_or(MnxError::Oom)?;
    unsafe {
        core::ptr::write_bytes(code, 0, total);
    }
    let image_va = code as u64;

    for i in 1..e_shnum {
        let sh = shdrs[i].as_ref().unwrap();
        if sh.flags & SHF_ALLOC == 0 || sh.typ != SHT_PROGBITS || sh.size == 0 {
            continue;
        }
        let src = sh.offset as usize;
        let n = sh.size as usize;
        if src.saturating_add(n) > buf.len() {
            kfree(code);
            return Err(MnxError::Truncated);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(buf[src..src + n].as_ptr(), code.add(sec_base[i] as usize), n);
        }
    }

    // Find SYMTAB.
    let mut symtab_i: Option<usize> = None;
    for i in 1..e_shnum {
        if shdrs[i].as_ref().unwrap().typ == SHT_SYMTAB {
            symtab_i = Some(i);
            break;
        }
    }
    let Some(sym_i) = symtab_i else {
        kfree(code);
        return Err(MnxError::BadCode);
    };
    let symtab = shdrs[sym_i].as_ref().unwrap();
    if symtab.link as usize >= e_shnum {
        kfree(code);
        return Err(MnxError::BadCode);
    }
    let strtab = shdrs[symtab.link as usize].as_ref().unwrap();
    if strtab.typ != SHT_STRTAB {
        kfree(code);
        return Err(MnxError::BadCode);
    }
    let nsyms = if symtab.entsize == 0 {
        0
    } else {
        (symtab.size / symtab.entsize) as usize
    };
    if nsyms > 256 || (symtab.entsize as usize) != SYMSIZE {
        kfree(code);
        return Err(MnxError::BadReloc);
    }

    let mut tramps = [Tramp {
        used: false,
        name: [0; 32],
        nlen: 0,
        addr: 0,
    }; MAX_TRAMP];
    let mut ntramp = 0usize;

    let mut get_or_make_tramp = |name: &str, dest: u64| -> Result<u64, MnxError> {
        for t in tramps.iter() {
            if tramp_matches(t, name) {
                return Ok(t.addr);
            }
        }
        if ntramp >= MAX_TRAMP || name.len() > 32 {
            return Err(MnxError::BadReloc);
        }
        let addr = image_va + tramp_off + (ntramp * TRAMP_STRIDE) as u64;
        emit_tramp(addr, dest);
        let t = &mut tramps[ntramp];
        t.used = true;
        t.nlen = name.len() as u8;
        t.name[..name.len()].copy_from_slice(name.as_bytes());
        t.addr = addr;
        ntramp += 1;
        Ok(addr)
    };

    let resolve = |sym: &Sym| -> Result<u64, MnxError> {
        if sym.shndx == SHN_UNDEF {
            Err(MnxError::Unresolved)
        } else if sym.shndx == SHN_ABS {
            Ok(sym.value)
        } else if sym.shndx == SHN_COMMON {
            Err(MnxError::BadReloc)
        } else {
            let si = sym.shndx as usize;
            if si >= e_shnum {
                return Err(MnxError::BadReloc);
            }
            let sh = shdrs[si].as_ref().ok_or(MnxError::BadReloc)?;
            if sh.flags & SHF_ALLOC == 0 {
                return Err(MnxError::BadReloc);
            }
            Ok(image_va + sec_base[si] + sym.value)
        }
    };

    // Apply RELA.
    for i in 1..e_shnum {
        let rela = shdrs[i].as_ref().unwrap();
        if rela.typ != SHT_RELA {
            continue;
        }
        let target = rela.info as usize;
        if target >= e_shnum {
            kfree(code);
            return Err(MnxError::BadReloc);
        }
        let tsh = shdrs[target].as_ref().unwrap();
        if tsh.flags & SHF_ALLOC == 0 {
            continue;
        }
        if rela.entsize as usize != RELASIZE || rela.link as usize != sym_i {
            kfree(code);
            return Err(MnxError::BadReloc);
        }
        let nrel = (rela.size / rela.entsize) as usize;
        if nrel > 256 {
            kfree(code);
            return Err(MnxError::BadReloc);
        }
        for r in 0..nrel {
            let ro = rela.offset as usize + r * RELASIZE;
            let r_offset = match read_u64(buf, ro) {
                Ok(v) => v,
                Err(e) => {
                    kfree(code);
                    return Err(e);
                }
            };
            let r_info = match read_u64(buf, ro + 8) {
                Ok(v) => v,
                Err(e) => {
                    kfree(code);
                    return Err(e);
                }
            };
            let r_addend = match read_i64(buf, ro + 16) {
                Ok(v) => v,
                Err(e) => {
                    kfree(code);
                    return Err(e);
                }
            };
            let r_type = (r_info & 0xffff_ffff) as u32;
            let r_sym = (r_info >> 32) as usize;
            if r_type == R_X86_64_NONE {
                continue;
            }
            if r_sym >= nsyms {
                kfree(code);
                return Err(MnxError::BadReloc);
            }
            let sym_off = symtab.offset as usize + r_sym * SYMSIZE;
            let sym = match parse_sym(buf, sym_off) {
                Ok(s) => s,
                Err(e) => {
                    kfree(code);
                    return Err(e);
                }
            };
            let sname = match strtab_get(buf, strtab, sym.name) {
                Ok(s) => s,
                Err(e) => {
                    kfree(code);
                    return Err(e);
                }
            };
            let p = image_va + sec_base[target] + r_offset;
            let loc = p as *mut u8;
            let is_pc = r_type == R_X86_64_PC32 || r_type == R_X86_64_PLT32;
            let s = if sym.shndx == SHN_UNDEF {
                let dest = match export::lookup(sname) {
                    Some(a) => a,
                    None => {
                        kfree(code);
                        return Err(MnxError::Unresolved);
                    }
                };
                if is_pc {
                    match get_or_make_tramp(sname, dest) {
                        Ok(t) => t,
                        Err(e) => {
                            kfree(code);
                            return Err(e);
                        }
                    }
                } else {
                    dest
                }
            } else {
                match resolve(&sym) {
                    Ok(v) => v,
                    Err(e) => {
                        kfree(code);
                        return Err(e);
                    }
                }
            };

            let ok = match r_type {
                R_X86_64_64 => {
                    let val = (s as i64).wrapping_add(r_addend) as u64;
                    unsafe {
                        core::ptr::write_unaligned(loc as *mut u64, val);
                    }
                    true
                }
                R_X86_64_PC32 | R_X86_64_PLT32 => {
                    let val = (s as i64)
                        .wrapping_add(r_addend)
                        .wrapping_sub(p as i64);
                    if val < i32::MIN as i64 || val > i32::MAX as i64 {
                        false
                    } else {
                        unsafe {
                            core::ptr::write_unaligned(loc as *mut i32, val as i32);
                        }
                        true
                    }
                }
                R_X86_64_32 | R_X86_64_32S => {
                    let val = (s as i64).wrapping_add(r_addend);
                    if r_type == R_X86_64_32 {
                        if val < 0 || val > u32::MAX as i64 {
                            false
                        } else {
                            unsafe {
                                core::ptr::write_unaligned(loc as *mut u32, val as u32);
                            }
                            true
                        }
                    } else if val < i32::MIN as i64 || val > i32::MAX as i64 {
                        false
                    } else {
                        unsafe {
                            core::ptr::write_unaligned(loc as *mut i32, val as i32);
                        }
                        true
                    }
                }
                _ => false,
            };
            if !ok {
                kfree(code);
                return Err(MnxError::BadReloc);
            }
        }
    }

    // Find init_module / cleanup_module.
    let mut init_va: Option<u64> = None;
    let mut exit_va: Option<u64> = None;
    for si in 1..nsyms {
        let sym_off = symtab.offset as usize + si * SYMSIZE;
        let sym = match parse_sym(buf, sym_off) {
            Ok(s) => s,
            Err(e) => {
                kfree(code);
                return Err(e);
            }
        };
        if sym.shndx == SHN_UNDEF || sym.shndx == SHN_ABS || sym.shndx == SHN_COMMON {
            continue;
        }
        let sname = match strtab_get(buf, strtab, sym.name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let va = match resolve(&sym) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match sname {
            "init_module" | "init" => init_va = Some(va),
            "cleanup_module" | "cleanup" => exit_va = Some(va),
            _ => {}
        }
    }
    let Some(init_va) = init_va else {
        kfree(code);
        return Err(MnxError::BadCode);
    };

    let mut name = [0u8; MNX_NAME_LEN];
    let name_len;
    if parsed_nlen > 0 {
        name = parsed_name;
        name_len = parsed_nlen;
    } else if !name_hint.is_empty() {
        let b = name_hint.as_bytes();
        let k = b.len().min(MNX_NAME_LEN);
        name[..k].copy_from_slice(&b[..k]);
        name_len = k;
    } else {
        let b = b"module";
        name[..b.len()].copy_from_slice(b);
        name_len = b.len();
    }

    let init: Option<extern "C" fn() -> i32> =
        Some(unsafe { core::mem::transmute(init_va as usize) });
    let exit: Option<extern "C" fn() -> i32> =
        exit_va.map(|v| unsafe { core::mem::transmute(v as usize) });

    Ok(LoadedMnx {
        name,
        name_len,
        code,
        code_len: total,
        init,
        exit,
    })
}
