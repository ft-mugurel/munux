//! ELF64 loader for freestanding x86_64 user programs (ET_EXEC).

use crate::memory::paging::{self, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::memory::pmm::{self, FRAME_SIZE, PhysAddr};

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;

const MAX_LOAD_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FILE_SIZE: usize = 2 * 1024 * 1024;

/// User stack grows down toward lower addresses.
pub const USER_STACK_TOP: u64 = 0x0000_0000_7FFF_F000;
const USER_STACK_PAGES: u64 = 4;

const PAGE_USER_RW: u64 = PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

#[repr(C)]
#[derive(Clone, Copy)]
struct Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct LoadedImage {
    pub entry: u64,
    pub stack_top: u64,
    /// Initial program break: page-aligned end of the highest PT_LOAD segment.
    /// Linux `brk` grows the heap from here.
    pub brk_start: u64,
}

fn page_down(a: u64) -> u64 {
    a & !(FRAME_SIZE as u64 - 1)
}
fn page_up(a: u64) -> u64 {
    (a + FRAME_SIZE as u64 - 1) & !(FRAME_SIZE as u64 - 1)
}

pub fn map_user_page(virt: u64) -> Result<(), &'static str> {
    if virt & 0xFFF != 0 {
        return Err("elf: page not aligned");
    }
    if virt < 0x1000 || virt >= 0x0000_8000_0000_0000 {
        return Err("elf: bad user VA");
    }
    if let Some(phys) = paging::virt_to_phys(virt) {
        let page = phys & !0xFFF;
        paging::map_page(virt, PhysAddr::new(page), PAGE_USER_RW);
        return Ok(());
    }
    let frame = pmm::alloc_frame().ok_or("elf: OOM page")?;
    paging::map_page(virt, frame, PAGE_USER_RW);
    Ok(())
}

fn map_user_range(start: u64, end: u64) -> Result<(), &'static str> {
    let mut v = page_down(start);
    let end = page_up(end);
    while v < end {
        map_user_page(v)?;
        v = v.wrapping_add(FRAME_SIZE as u64);
    }
    Ok(())
}

fn write_user(virt: u64, src: &[u8]) -> Result<(), &'static str> {
    if src.is_empty() {
        return Ok(());
    }
    let end = virt.saturating_add(src.len() as u64);
    if virt < 0x1000 || end > 0x0000_8000_0000_0000 {
        return Err("elf: write outside user space");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), virt as *mut u8, src.len());
    }
    Ok(())
}

fn zero_user(virt: u64, len: u64) -> Result<(), &'static str> {
    if len == 0 {
        return Ok(());
    }
    let end = virt.saturating_add(len);
    if virt < 0x1000 || end > 0x0000_8000_0000_0000 {
        return Err("elf: zero outside user space");
    }
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, len as usize);
    }
    Ok(())
}

fn read_ehdr(file: &[u8]) -> Result<Ehdr, &'static str> {
    if file.len() < core::mem::size_of::<Ehdr>() {
        return Err("elf: truncated header");
    }
    let ehdr = unsafe { core::ptr::read_unaligned(file.as_ptr() as *const Ehdr) };
    Ok(ehdr)
}

fn validate_ehdr(h: &Ehdr) -> Result<(), &'static str> {
    if h.e_ident[0..4] != ELFMAG {
        return Err("elf: bad magic");
    }
    if h.e_ident[4] != ELFCLASS64 {
        return Err("elf: not ELF64");
    }
    if h.e_ident[5] != ELFDATA2LSB {
        return Err("elf: not little-endian");
    }
    if h.e_ident[6] != EV_CURRENT {
        return Err("elf: bad version");
    }
    if h.e_type != ET_EXEC {
        return Err("elf: need ET_EXEC");
    }
    if h.e_machine != EM_X86_64 {
        return Err("elf: need EM_X86_64");
    }
    if h.e_phentsize as usize != core::mem::size_of::<Phdr>() {
        return Err("elf: bad phentsize");
    }
    if h.e_phnum == 0 || h.e_phnum > 64 {
        return Err("elf: bad phnum");
    }
    if h.e_entry < 0x1000 || h.e_entry >= 0x0000_8000_0000_0000 {
        return Err("elf: bad entry");
    }
    Ok(())
}

fn read_phdr(file: &[u8], phoff: u64, index: u16) -> Result<Phdr, &'static str> {
    let off = phoff as usize + index as usize * core::mem::size_of::<Phdr>();
    if off + core::mem::size_of::<Phdr>() > file.len() {
        return Err("elf: truncated phdr");
    }
    Ok(unsafe { core::ptr::read_unaligned(file.as_ptr().add(off) as *const Phdr) })
}

fn load_segment(file: &[u8], ph: &Phdr) -> Result<u64, &'static str> {
    if ph.p_memsz == 0 {
        return Ok(0);
    }
    if ph.p_filesz > ph.p_memsz {
        return Err("elf: filesz > memsz");
    }
    if ph.p_vaddr < 0x1000 || ph.p_vaddr >= 0x0000_8000_0000_0000 {
        return Err("elf: bad p_vaddr");
    }
    let vend = ph.p_vaddr.saturating_add(ph.p_memsz);
    if vend > 0x0000_8000_0000_0000 {
        return Err("elf: segment past user space");
    }
    if ph.p_filesz > 0 {
        let fend = ph.p_offset as usize + ph.p_filesz as usize;
        if fend > file.len() {
            return Err("elf: segment past EOF");
        }
    }
    if ph.p_memsz > MAX_LOAD_BYTES {
        return Err("elf: segment too large");
    }

    map_user_range(ph.p_vaddr, ph.p_vaddr.saturating_add(ph.p_memsz))?;

    if ph.p_filesz > 0 {
        let start = ph.p_offset as usize;
        let end = start + ph.p_filesz as usize;
        write_user(ph.p_vaddr, &file[start..end])?;
    }
    if ph.p_memsz > ph.p_filesz {
        zero_user(ph.p_vaddr + ph.p_filesz, ph.p_memsz - ph.p_filesz)?;
    }
    Ok(ph.p_memsz)
}

/// Minimal ELF info for the initial auxiliary vector (Linux process startup).
#[derive(Clone, Copy)]
pub struct AuxInfo {
    pub entry: u64,
    /// Virtual address of the program header table in the loaded image (0 if unknown).
    pub phdr: u64,
    pub phent: u64,
    pub phnum: u64,
}

// Linux uapi/linux/auxvec.h
const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_SECURE: u64 = 23;
const AT_RANDOM: u64 = 25;

/// Build a Linux-like initial stack:
/// `[argc][argv…][NULL][envp NULL][auxv… AT_NULL][strings][16B random]`
///
/// `argv` strings are copied onto the stack (max 4 args, 64 bytes each).
/// Auxv is required by musl's `__init_libc` (it walks pairs until `AT_NULL`).
pub fn setup_stack(argv: &[&str], aux: &AuxInfo) -> Result<u64, &'static str> {
    let stack_base = USER_STACK_TOP - USER_STACK_PAGES * FRAME_SIZE as u64;
    map_user_range(stack_base, USER_STACK_TOP)?;
    for i in 0..USER_STACK_PAGES {
        zero_user(stack_base + i * FRAME_SIZE as u64, FRAME_SIZE as u64)?;
    }

    let narg = core::cmp::min(argv.len(), 4);
    if narg == 0 {
        return Err("elf: empty argv");
    }

    // High end: 16 bytes for AT_RANDOM, then argv strings.
    let mut top = USER_STACK_TOP;
    top -= 16;
    top &= !0xF;
    let random_ptr = top;
    // Pseudo-random enough for stack canaries / musl (not crypto).
    let rnd: [u8; 16] = [
        0x6d, 0x75, 0x6e, 0x75, 0x78, 0xa5, 0x5a, 0xc3, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde,
        0xf0,
    ];
    write_user(random_ptr, &rnd)?;

    let mut str_ptrs = [0u64; 4];
    for i in 0..narg {
        let bytes = argv[i].as_bytes();
        let len = core::cmp::min(bytes.len(), 64);
        top -= (len as u64) + 1;
        top &= !7; // 8-byte align
        write_user(top, &bytes[..len])?;
        write_user(top + len as u64, &[0u8])?;
        str_ptrs[i] = top;
    }

    // Aux vector entries: (type, value) pairs, terminated by (AT_NULL, 0).
    // Count pairs including AT_NULL.
    let mut aux_pairs: [(u64, u64); 16] = [(0, 0); 16];
    let mut naux = 0usize;
    let push_aux = |pairs: &mut [(u64, u64); 16], n: &mut usize, t: u64, v: u64| {
        if *n < pairs.len() {
            pairs[*n] = (t, v);
            *n += 1;
        }
    };
    push_aux(&mut aux_pairs, &mut naux, AT_PAGESZ, FRAME_SIZE as u64);
    if aux.phdr != 0 {
        push_aux(&mut aux_pairs, &mut naux, AT_PHDR, aux.phdr);
        push_aux(&mut aux_pairs, &mut naux, AT_PHENT, aux.phent);
        push_aux(&mut aux_pairs, &mut naux, AT_PHNUM, aux.phnum);
    }
    push_aux(&mut aux_pairs, &mut naux, AT_BASE, 0); // static ET_EXEC
    push_aux(&mut aux_pairs, &mut naux, AT_FLAGS, 0);
    push_aux(&mut aux_pairs, &mut naux, AT_ENTRY, aux.entry);
    push_aux(&mut aux_pairs, &mut naux, AT_UID, 0);
    push_aux(&mut aux_pairs, &mut naux, AT_EUID, 0);
    push_aux(&mut aux_pairs, &mut naux, AT_GID, 0);
    push_aux(&mut aux_pairs, &mut naux, AT_EGID, 0);
    push_aux(&mut aux_pairs, &mut naux, AT_SECURE, 0);
    push_aux(&mut aux_pairs, &mut naux, AT_RANDOM, random_ptr);
    push_aux(&mut aux_pairs, &mut naux, AT_NULL, 0);

    // Words below strings:
    // argc + narg argv ptrs + argv NULL + env NULL + naux*(type,value)
    let words = 1 + narg + 1 + 1 + naux * 2;
    let mut sp = top - (words as u64) * 8;
    sp &= !0xF;

    let mut off = 0u64;
    write_user(sp + off, &(narg as u64).to_le_bytes())?;
    off += 8;
    for i in 0..narg {
        write_user(sp + off, &str_ptrs[i].to_le_bytes())?;
        off += 8;
    }
    write_user(sp + off, &0u64.to_le_bytes())?; // argv NULL
    off += 8;
    write_user(sp + off, &0u64.to_le_bytes())?; // envp NULL
    off += 8;
    for i in 0..naux {
        let (t, v) = aux_pairs[i];
        write_user(sp + off, &t.to_le_bytes())?;
        off += 8;
        write_user(sp + off, &v.to_le_bytes())?;
        off += 8;
    }
    let _ = off;

    Ok(sp)
}

/// Load ELF64 bytes into user memory and prepare stack with argv0 only.
pub fn load_bytes(file: &[u8], argv0: &str) -> Result<LoadedImage, &'static str> {
    load_bytes_argv(file, &[argv0])
}

/// Load ELF64 and set up stack with full argv.
pub fn load_bytes_argv(file: &[u8], argv: &[&str]) -> Result<LoadedImage, &'static str> {
    if file.len() > MAX_FILE_SIZE {
        return Err("elf: file too large");
    }
    let ehdr = read_ehdr(file)?;
    validate_ehdr(&ehdr)?;

    let mut total = 0u64;
    let mut image_end = 0u64;
    // VA of program headers once loaded (segment that contains e_phoff).
    let mut phdr_va = 0u64;
    for i in 0..ehdr.e_phnum {
        let ph = read_phdr(file, ehdr.e_phoff, i)?;
        if ph.p_type != PT_LOAD {
            continue;
        }
        total = total.saturating_add(load_segment(file, &ph)?);
        if total > MAX_LOAD_BYTES {
            return Err("elf: image too large");
        }
        let vend = ph.p_vaddr.saturating_add(ph.p_memsz);
        if vend > image_end {
            image_end = vend;
        }
        // If this LOAD covers the file offset of the phdr table, compute its VA.
        let ph_end = ehdr.e_phoff.saturating_add(
            (ehdr.e_phnum as u64).saturating_mul(ehdr.e_phentsize as u64),
        );
        if phdr_va == 0
            && ehdr.e_phoff >= ph.p_offset
            && ph_end <= ph.p_offset.saturating_add(ph.p_filesz)
        {
            phdr_va = ph.p_vaddr.saturating_add(ehdr.e_phoff - ph.p_offset);
        }
    }
    if total == 0 {
        return Err("elf: no PT_LOAD segments");
    }

    // Program break starts at the first page boundary at/after the image end.
    let brk_start = page_up(image_end);
    if brk_start < 0x1000 || brk_start >= 0x0000_8000_0000_0000 {
        return Err("elf: bad brk start");
    }

    let aux = AuxInfo {
        entry: ehdr.e_entry,
        phdr: phdr_va,
        phent: ehdr.e_phentsize as u64,
        phnum: ehdr.e_phnum as u64,
    };
    let stack_top = setup_stack(argv, &aux)?;
    Ok(LoadedImage {
        entry: ehdr.e_entry,
        stack_top,
        brk_start,
    })
}
