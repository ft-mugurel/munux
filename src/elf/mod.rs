//! ELF64 loader for freestanding x86_64 user programs (ET_EXEC).

use crate::memory::paging::{self, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::memory::pmm::{self, FRAME_SIZE};

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const PT_INTERP: u32 = 3;

/// Cap on sum of PT_LOAD memsz (inode-backed loads; no 2 MiB kernel scratch).
const MAX_LOAD_BYTES: u64 = 16 * 1024 * 1024;
/// In-memory / embedded images still fit in a small buffer.
const MAX_FILE_SIZE: usize = 2 * 1024 * 1024;

/// User stack grows down toward lower addresses.
pub const USER_STACK_TOP: u64 = 0x0000_0000_7FFF_F000;
/// Default stack for non-fork loads. BusyBox can use hundreds of KiB.
const USER_STACK_PAGES: u64 = 256; // 1 MiB

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
    // Low ET_EXEC window: promote whole 2 MiB identity pages to user.
    if virt >= IDENTITY_USER_LO && virt < IDENTITY_USER_HI {
        let base = virt & !0x1F_FFFF;
        return paging::map_identity_2m_user(base);
    }
    // High user stack / other: private frames.
    if paging::virt_to_phys(virt).is_some() {
        return Ok(());
    }
    let frame = pmm::alloc_frame().ok_or("elf: OOM page")?;
    paging::map_page(virt, frame, PAGE_USER_RW);
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE);
    }
    Ok(())
}

fn map_user_range(start: u64, end: u64) -> Result<(), &'static str> {
    let mut v = page_down(start);
    let end = page_up(end);
    // Coalesce low identity range into 2 MiB promotions.
    if v >= IDENTITY_USER_LO && end <= IDENTITY_USER_HI {
        let mut b = v & !0x1F_FFFF;
        let last = (end - 1) & !0x1F_FFFF;
        while b <= last {
            paging::map_identity_2m_user(b)?;
            b = b.saturating_add(0x20_0000);
            if b == 0 {
                break;
            }
        }
        return Ok(());
    }
    while v < end {
        map_user_page(v)?;
        v = v.wrapping_add(FRAME_SIZE as u64);
    }
    Ok(())
}

/// Classic static ET_EXEC window lives in the low identity map (VA == PA).
/// Writes go straight to that physical memory; page tables only need U/S for
/// user-mode fetch, not for the kernel copy path.
const IDENTITY_USER_LO: u64 = 0x400000;
const IDENTITY_USER_HI: u64 = 0x8000000; // 128 MiB

fn write_user(virt: u64, src: &[u8]) -> Result<(), &'static str> {
    if src.is_empty() {
        return Ok(());
    }
    let end = virt.saturating_add(src.len() as u64);
    if virt < 0x1000 || end > 0x0000_8000_0000_0000 {
        return Err("elf: write outside user space");
    }
    // Preferred path: ET_EXEC image in identity window (BusyBox, musl, …).
    if virt >= IDENTITY_USER_LO && end <= IDENTITY_USER_HI {
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), virt as *mut u8, src.len());
        }
        return Ok(());
    }
    // High user stack / other: write via resolved PA (identity for low frames).
    let mut done = 0usize;
    while done < src.len() {
        let va = virt + done as u64;
        let page = va & !0xFFF;
        let phys_base = paging::virt_to_phys(page).ok_or("elf: write unmapped")?;
        let phys_page = phys_base & !0xFFF;
        let page_off = (va & 0xFFF) as usize;
        let chunk = core::cmp::min(src.len() - done, FRAME_SIZE - page_off);
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr().add(done),
                (phys_page as usize + page_off) as *mut u8,
                chunk,
            );
        }
        done += chunk;
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
    if virt >= IDENTITY_USER_LO && end <= IDENTITY_USER_HI {
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, len as usize);
        }
        return Ok(());
    }
    let mut done = 0u64;
    while done < len {
        let va = virt + done;
        let page = va & !0xFFF;
        let phys_base = paging::virt_to_phys(page).ok_or("elf: zero unmapped")?;
        let phys_page = phys_base & !0xFFF;
        let page_off = va & 0xFFF;
        let chunk = core::cmp::min(len - done, FRAME_SIZE as u64 - page_off);
        unsafe {
            core::ptr::write_bytes((phys_page + page_off) as *mut u8, 0, chunk as usize);
        }
        done += chunk;
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
    /// Interpreter load address (`AT_BASE`); 0 if statically linked.
    pub base: u64,
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

/// Choose stack top for a new image.
///
/// Under cooperative fork with a **shared** address space, a forked child has a
/// private stack at `PCB.stack_base`. Exec must rebuild argv there — never at
/// `USER_STACK_TOP` — or it zeroes the sleeping parent's shell stack.
fn exec_stack_top() -> (u64, u64) {
    if let Some((base, size)) = crate::process::current_stack_region() {
        if base >= 0x1000 && size >= FRAME_SIZE as u64 {
            let top = base.saturating_add(size);
            // Leave top page-aligned high address (same convention as USER_STACK_TOP).
            return (top, size / FRAME_SIZE as u64);
        }
    }
    (USER_STACK_TOP, USER_STACK_PAGES)
}

/// Build a Linux-like initial stack:
/// `[argc][argv…][NULL][envp NULL][auxv… AT_NULL][strings][16B random]`
///
/// `argv` strings are copied onto the stack (max 6 args, 64 bytes each).
/// Auxv is required by musl's `__init_libc` (it walks pairs until `AT_NULL`).
pub fn setup_stack(argv: &[&str], aux: &AuxInfo) -> Result<u64, &'static str> {
    let (stack_top, pages) = exec_stack_top();
    // Cap at 256 pages (1 MiB) — matches child-stack sizing for BusyBox.
    let pages = pages.max(1).min(256);
    let stack_base = stack_top - pages * FRAME_SIZE as u64;
    // Map [base, top) plus one page at `stack_top` itself. BusyBox/musl has
    // been observed to touch a few dozen bytes just above the initial SP
    // region (CR2 ≈ stack_top+0x42 → not-present #PF after touch/mkdir).
    // Child stacks use a 2 MiB stride with 1 MiB used, so +4 KiB fits.
    let map_end = stack_top.saturating_add(FRAME_SIZE as u64);
    map_user_range(stack_base, map_end)?;
    for i in 0..=pages {
        zero_user(stack_base + i * FRAME_SIZE as u64, FRAME_SIZE as u64)?;
    }

    let narg = core::cmp::min(argv.len(), 6);
    if narg == 0 {
        return Err("elf: empty argv");
    }

    // High end: 16 bytes for AT_RANDOM, then argv strings.
    let mut top = stack_top;
    top -= 16;
    top &= !0xF;
    let random_ptr = top;
    // Pseudo-random enough for stack canaries / musl (not crypto).
    let rnd: [u8; 16] = [
        0x6d, 0x75, 0x6e, 0x75, 0x78, 0xa5, 0x5a, 0xc3, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde,
        0xf0,
    ];
    write_user(random_ptr, &rnd)?;

    let mut str_ptrs = [0u64; 6];
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
    push_aux(&mut aux_pairs, &mut naux, AT_BASE, aux.base);
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

    // Sanity: entry page must be present (catches failed private-frame installs).
    {
        let entry = ehdr.e_entry;
        let page = entry & !0xFFF;
        match paging::virt_to_phys(page) {
            None => return Err("elf: entry page unmapped"),
            Some(_) => {
                let b0 = unsafe { core::ptr::read_volatile(entry as *const u8) };
                let b1 = unsafe { core::ptr::read_volatile((entry + 1) as *const u8) };
                let b2 = unsafe { core::ptr::read_volatile((entry + 2) as *const u8) };
                if b0 == 0 && b1 == 0 && b2 == 0 {
                    return Err("elf: entry page zero");
                }
            }
        }
    }

    let aux = AuxInfo {
        entry: ehdr.e_entry,
        phdr: phdr_va,
        phent: ehdr.e_phentsize as u64,
        phnum: ehdr.e_phnum as u64,
        base: 0,
    };
    let stack_top = setup_stack(argv, &aux)?;
    Ok(LoadedImage {
        entry: ehdr.e_entry,
        stack_top,
        brk_start,
    })
}

fn read_ino(ino: u32, off: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
    if off > u32::MAX as u64 {
        return Err("elf: offset too large");
    }
    crate::fs::ext2::read_file(ino, off as u32, buf)
}

fn load_segment_ino(ino: u32, file_size: u64, ph: &Phdr) -> Result<u64, &'static str> {
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
        let fend = ph.p_offset.saturating_add(ph.p_filesz);
        if fend > file_size {
            return Err("elf: segment past EOF");
        }
    }
    if ph.p_memsz > MAX_LOAD_BYTES {
        return Err("elf: segment too large");
    }

    map_user_range(ph.p_vaddr, ph.p_vaddr.saturating_add(ph.p_memsz))?;

    if ph.p_filesz > 0 {
        let mut done = 0u64;
        let mut tmp = [0u8; 512];
        while done < ph.p_filesz {
            let chunk = core::cmp::min((ph.p_filesz - done) as usize, tmp.len());
            let n = read_ino(ino, ph.p_offset + done, &mut tmp[..chunk])?;
            if n == 0 {
                return Err("elf: short segment read");
            }
            write_user(ph.p_vaddr + done, &tmp[..n])?;
            done += n as u64;
        }
    }
    if ph.p_memsz > ph.p_filesz {
        zero_user(ph.p_vaddr + ph.p_filesz, ph.p_memsz - ph.p_filesz)?;
    }
    Ok(ph.p_memsz)
}

struct ElfHeaders {
    ehdr: Ehdr,
    phbuf: [u8; 4096],
    file_size: u64,
}

struct ImageParts {
    entry: u64,
    image_end: u64,
    phdr_va: u64,
    load_base: u64,
}

fn parse_headers(ino: u32) -> Result<ElfHeaders, &'static str> {
    if crate::fs::ext2::inode_is_dir(ino) {
        return Err("is a directory");
    }
    let file_size = crate::fs::ext2::inode_file_size(ino) as u64;
    if file_size < 64 {
        return Err("elf: short read");
    }
    let mut ehbuf = [0u8; 64];
    let n = read_ino(ino, 0, &mut ehbuf)?;
    if n < 64 {
        return Err("elf: truncated header");
    }
    let ehdr = read_ehdr(&ehbuf)?;
    validate_ehdr(&ehdr)?;
    let phoff = ehdr.e_phoff;
    let phnum = ehdr.e_phnum as usize;
    let phentsz = ehdr.e_phentsize as usize;
    if phentsz != core::mem::size_of::<Phdr>() {
        return Err("elf: bad phentsize");
    }
    let ph_bytes = phnum.saturating_mul(phentsz);
    if ph_bytes == 0 || ph_bytes > 4096 {
        return Err("elf: bad phnum");
    }
    let mut phbuf = [0u8; 4096];
    let got = read_ino(ino, phoff, &mut phbuf[..ph_bytes])?;
    if got < ph_bytes {
        return Err("elf: truncated phdr");
    }
    Ok(ElfHeaders {
        ehdr,
        phbuf,
        file_size,
    })
}

fn phdr_at(h: &ElfHeaders, i: u16) -> Phdr {
    let off = i as usize * h.ehdr.e_phentsize as usize;
    unsafe { core::ptr::read_unaligned(h.phbuf.as_ptr().add(off) as *const Phdr) }
}

fn find_interp_path(ino: u32, h: &ElfHeaders) -> Result<Option<([u8; 128], usize)>, &'static str> {
    for i in 0..h.ehdr.e_phnum {
        let ph = phdr_at(h, i);
        if ph.p_type != PT_INTERP {
            continue;
        }
        if ph.p_filesz == 0 || ph.p_filesz > 127 {
            return Err("elf: bad PT_INTERP");
        }
        let mut buf = [0u8; 128];
        let n = read_ino(ino, ph.p_offset, &mut buf[..ph.p_filesz as usize])?;
        if n == 0 {
            return Err("elf: empty PT_INTERP");
        }
        let mut len = 0usize;
        while len < n && buf[len] != 0 {
            len += 1;
        }
        if len == 0 {
            return Err("elf: empty PT_INTERP");
        }
        return Ok(Some((buf, len)));
    }
    Ok(None)
}

fn check_entry_mapped(entry: u64) -> Result<(), &'static str> {
    let page = entry & !0xFFF;
    match paging::virt_to_phys(page) {
        None => Err("elf: entry page unmapped"),
        Some(_) => {
            let b0 = unsafe { core::ptr::read_volatile(entry as *const u8) };
            let b1 = unsafe { core::ptr::read_volatile((entry + 1) as *const u8) };
            let b2 = unsafe { core::ptr::read_volatile((entry + 2) as *const u8) };
            if b0 == 0 && b1 == 0 && b2 == 0 {
                Err("elf: entry page zero")
            } else {
                Ok(())
            }
        }
    }
}

fn load_parts_ino(ino: u32, h: &ElfHeaders) -> Result<ImageParts, &'static str> {
    let mut total = 0u64;
    let mut image_end = 0u64;
    let mut phdr_va = 0u64;
    let mut load_base = u64::MAX;
    for i in 0..h.ehdr.e_phnum {
        let ph = phdr_at(h, i);
        if ph.p_type != PT_LOAD {
            continue;
        }
        total = total.saturating_add(load_segment_ino(ino, h.file_size, &ph)?);
        if total > MAX_LOAD_BYTES {
            return Err("elf: image too large");
        }
        if ph.p_vaddr < load_base {
            load_base = ph.p_vaddr;
        }
        let vend = ph.p_vaddr.saturating_add(ph.p_memsz);
        if vend > image_end {
            image_end = vend;
        }
        let ph_end = h.ehdr.e_phoff.saturating_add(
            (h.ehdr.e_phnum as u64).saturating_mul(h.ehdr.e_phentsize as u64),
        );
        if phdr_va == 0
            && h.ehdr.e_phoff >= ph.p_offset
            && ph_end <= ph.p_offset.saturating_add(ph.p_filesz)
        {
            phdr_va = ph.p_vaddr.saturating_add(h.ehdr.e_phoff - ph.p_offset);
        }
    }
    if total == 0 {
        return Err("elf: no PT_LOAD segments");
    }
    if load_base == u64::MAX {
        load_base = 0;
    }
    check_entry_mapped(h.ehdr.e_entry)?;
    Ok(ImageParts {
        entry: h.ehdr.e_entry,
        image_end,
        phdr_va,
        load_base,
    })
}

/// Load ET_EXEC from an ext2 inode: headers + each `PT_LOAD`.
///
/// If the file has `PT_INTERP`, the interpreter is loaded too (P10a). The
/// returned `entry` is the **interpreter** entry; `AT_ENTRY` on the stack is
/// still the main binary so a real `ld.so` (or our smoke interp) can jump there.
pub fn load_from_ino(ino: u32, argv: &[&str]) -> Result<LoadedImage, &'static str> {
    if !crate::fs::is_ready() {
        return Err("no filesystem");
    }
    let headers = parse_headers(ino)?;
    let interp = find_interp_path(ino, &headers)?;
    let main = load_parts_ino(ino, &headers)?;

    let (run_entry, at_base) = if let Some((raw, len)) = interp {
        let path = core::str::from_utf8(&raw[..len]).map_err(|_| "elf: bad PT_INTERP")?;
        let cwd = crate::fs::path::cwd_inode();
        let iino = crate::fs::ext2::resolve_path(cwd, path).map_err(|_| "not found")?;
        let ih = parse_headers(iino)?;
        let ip = load_parts_ino(iino, &ih)?;
        (ip.entry, ip.load_base)
    } else {
        (main.entry, 0)
    };

    let brk_start = page_up(main.image_end);
    if brk_start < 0x1000 || brk_start >= 0x0000_8000_0000_0000 {
        return Err("elf: bad brk start");
    }

    let aux = AuxInfo {
        entry: main.entry,
        phdr: main.phdr_va,
        phent: headers.ehdr.e_phentsize as u64,
        phnum: headers.ehdr.e_phnum as u64,
        base: at_base,
    };
    let stack_top = setup_stack(argv, &aux)?;
    Ok(LoadedImage {
        entry: run_entry,
        stack_top,
        brk_start,
    })
}
