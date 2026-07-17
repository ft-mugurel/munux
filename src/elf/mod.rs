//! ELF32 loader for freestanding i386 user programs (ET_EXEC).

use crate::fs::ext2;
use crate::memory::paging::{self, PAGE_USER_RW};
use crate::memory::pmm::{self, FRAME_SIZE, PhysAddr};

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS32: u8 = 1;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_386: u16 = 3;
const PT_LOAD: u32 = 1;

const EI_NIDENT: usize = 16;

/// Soft cap: refuse images that would map more than this many bytes of VA.
const MAX_LOAD_BYTES: u32 = 4 * 1024 * 1024;
/// Max ELF file size we will stream from disk.
const MAX_FILE_SIZE: u32 = 2 * 1024 * 1024;

/// Classic high user stack (grows down toward lower addresses).
pub const USER_STACK_TOP: u32 = 0xC000_0000;
const USER_STACK_PAGES: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
struct Ehdr {
    e_ident: [u8; EI_NIDENT],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u32,
    e_phoff: u32,
    e_shoff: u32,
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
    p_offset: u32,
    p_vaddr: u32,
    p_paddr: u32,
    p_filesz: u32,
    p_memsz: u32,
    p_flags: u32,
    p_align: u32,
}

/// Result of a successful load — ready for `enter_user_mode`.
#[derive(Clone, Copy, Debug)]
pub struct LoadedImage {
    pub entry: u32,
    pub stack_top: u32,
}

fn page_down(a: u32) -> u32 {
    a & !(FRAME_SIZE as u32 - 1)
}

fn page_up(a: u32) -> u32 {
    (a + FRAME_SIZE as u32 - 1) & !(FRAME_SIZE as u32 - 1)
}

/// Ensure one user R/W page is mapped at `virt` (page-aligned).
pub fn map_user_page(virt: u32) -> Result<(), &'static str> {
    if virt & 0xFFF != 0 {
        return Err("elf: page not aligned");
    }
    // Keep out of kernel VA and null page
    if virt < 0x1000 || virt >= 0xC000_0000 {
        return Err("elf: bad user VA");
    }
    if let Some(info) = paging::get_page(virt) {
        if info.present {
            paging::map_page(virt, PhysAddr::new(info.phys), PAGE_USER_RW);
            return Ok(());
        }
    }
    let frame = pmm::alloc_frame().ok_or("elf: OOM page")?;
    unsafe {
        core::ptr::write_bytes(frame.as_u32() as *mut u8, 0, FRAME_SIZE);
    }
    paging::map_page(virt, frame, PAGE_USER_RW);
    Ok(())
}

fn map_user_range(start: u32, end: u32) -> Result<(), &'static str> {
    let mut v = page_down(start);
    let end = page_up(end);
    while v < end {
        map_user_page(v)?;
        v = v.wrapping_add(FRAME_SIZE as u32);
    }
    Ok(())
}

fn write_user(virt: u32, src: &[u8]) -> Result<(), &'static str> {
    // Kernel writes through the page mapping (supervisor can access user pages).
    if src.is_empty() {
        return Ok(());
    }
    let end = (virt as u64).saturating_add(src.len() as u64);
    if virt < 0x1000 || end > 0xC000_0000 {
        return Err("elf: write outside user space");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), virt as *mut u8, src.len());
    }
    Ok(())
}

fn zero_user(virt: u32, len: u32) -> Result<(), &'static str> {
    if len == 0 {
        return Ok(());
    }
    let end = (virt as u64).saturating_add(len as u64);
    if virt < 0x1000 || end > 0xC000_0000 {
        return Err("elf: zero outside user space");
    }
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, len as usize);
    }
    Ok(())
}

fn read_ehdr(ino: u32) -> Result<Ehdr, &'static str> {
    let mut buf = [0u8; core::mem::size_of::<Ehdr>()];
    let n = ext2::read_file(ino, 0, &mut buf)?;
    if n < buf.len() {
        return Err("elf: truncated header");
    }
    // Safety: plain bytes, fixed layout, little-endian host matches file
    let ehdr = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Ehdr) };
    Ok(ehdr)
}

fn validate_ehdr(h: &Ehdr) -> Result<(), &'static str> {
    if h.e_ident[0..4] != ELFMAG {
        return Err("elf: bad magic");
    }
    if h.e_ident[4] != ELFCLASS32 {
        return Err("elf: not ELF32");
    }
    if h.e_ident[5] != ELFDATA2LSB {
        return Err("elf: not little-endian");
    }
    if h.e_ident[6] != EV_CURRENT {
        return Err("elf: bad version");
    }
    if h.e_type != ET_EXEC {
        return Err("elf: need ET_EXEC (static linked)");
    }
    if h.e_machine != EM_386 {
        return Err("elf: need EM_386");
    }
    if h.e_phentsize as usize != core::mem::size_of::<Phdr>() {
        return Err("elf: bad phentsize");
    }
    if h.e_phnum == 0 || h.e_phnum > 64 {
        return Err("elf: bad phnum");
    }
    if h.e_entry < 0x1000 || h.e_entry >= 0xC000_0000 {
        return Err("elf: bad entry");
    }
    Ok(())
}

fn read_phdr(ino: u32, phoff: u32, index: u16) -> Result<Phdr, &'static str> {
    let off = phoff.saturating_add(index as u32 * core::mem::size_of::<Phdr>() as u32);
    let mut buf = [0u8; core::mem::size_of::<Phdr>()];
    let n = ext2::read_file(ino, off, &mut buf)?;
    if n < buf.len() {
        return Err("elf: truncated phdr");
    }
    Ok(unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Phdr) })
}

fn load_segment(ino: u32, file_size: u32, ph: &Phdr) -> Result<u32, &'static str> {
    if ph.p_memsz == 0 {
        return Ok(0);
    }
    if ph.p_filesz > ph.p_memsz {
        return Err("elf: filesz > memsz");
    }
    if ph.p_vaddr < 0x1000 || ph.p_vaddr >= 0xC000_0000 {
        return Err("elf: bad p_vaddr");
    }
    let vend = (ph.p_vaddr as u64).saturating_add(ph.p_memsz as u64);
    if vend > 0xC000_0000 {
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

    // Stream file contents into VA in small chunks
    let mut done = 0u32;
    let mut chunk = [0u8; 1024];
    while done < ph.p_filesz {
        let want = core::cmp::min(chunk.len() as u32, ph.p_filesz - done) as usize;
        let n = ext2::read_file(ino, ph.p_offset + done, &mut chunk[..want])?;
        if n == 0 {
            return Err("elf: short read");
        }
        write_user(ph.p_vaddr + done, &chunk[..n])?;
        done += n as u32;
    }

    // BSS
    if ph.p_memsz > ph.p_filesz {
        zero_user(ph.p_vaddr + ph.p_filesz, ph.p_memsz - ph.p_filesz)?;
    }
    Ok(ph.p_memsz)
}

fn setup_stack(argv0: &str) -> Result<u32, &'static str> {
    let stack_base = USER_STACK_TOP - USER_STACK_PAGES * FRAME_SIZE as u32;
    map_user_range(stack_base, USER_STACK_TOP)?;
    // zero stack pages
    for i in 0..USER_STACK_PAGES {
        zero_user(stack_base + i * FRAME_SIZE as u32, FRAME_SIZE as u32)?;
    }

    // Build a minimal Linux-style i386 user stack (grows down):
    //   [argc][argv0][NULL][env NULL][...strings...]
    // Strings sit just below USER_STACK_TOP; pointers below that.
    let name = argv0.as_bytes();
    let name_len = core::cmp::min(name.len(), 64);
    let str_addr = USER_STACK_TOP - (name_len as u32 + 1);
    // align down string region
    let str_addr = str_addr & !3;
    write_user(str_addr, &name[..name_len])?;
    write_user(str_addr + name_len as u32, &[0u8])?;

    // pointers: argc, argv[0], argv NULL, env NULL  (4 * 4 = 16)
    let mut sp = str_addr - 16;
    sp &= !0xF; // 16-byte align (harmless on i386, nice for future)

    // layout at sp:
    // +0  argc = 1
    // +4  argv[0] = str_addr
    // +8  NULL
    // +12 NULL (envp terminator)
    let words: [u32; 4] = [1, str_addr, 0, 0];
    for (i, w) in words.iter().enumerate() {
        write_user(sp + (i as u32) * 4, &w.to_le_bytes())?;
    }
    Ok(sp)
}

/// Load an ELF32 executable from an ext2 inode and prepare user stack.
pub fn load_inode(ino: u32, argv0: &str) -> Result<LoadedImage, &'static str> {
    let file_size = ext2::inode_file_size(ino);
    if file_size < core::mem::size_of::<Ehdr>() as u32 {
        return Err("elf: file too small");
    }
    if file_size > MAX_FILE_SIZE {
        return Err("elf: file too large");
    }
    if ext2::inode_is_dir(ino) {
        return Err("elf: is a directory");
    }

    let ehdr = read_ehdr(ino)?;
    validate_ehdr(&ehdr)?;

    let mut total = 0u32;
    for i in 0..ehdr.e_phnum {
        let ph = read_phdr(ino, ehdr.e_phoff, i)?;
        if ph.p_type != PT_LOAD {
            continue;
        }
        total = total.saturating_add(load_segment(ino, file_size, &ph)?);
        if total > MAX_LOAD_BYTES {
            return Err("elf: image too large");
        }
    }
    if total == 0 {
        return Err("elf: no PT_LOAD segments");
    }

    let stack_top = setup_stack(argv0)?;
    Ok(LoadedImage {
        entry: ehdr.e_entry,
        stack_top,
    })
}

/// Resolve path, load ELF, return image (does not enter user mode).
pub fn load_path(path: &str) -> Result<LoadedImage, &'static str> {
    if !crate::fs::is_ready() {
        return Err("elf: no filesystem");
    }
    let cwd = crate::fs::path::cwd_inode();
    let ino = ext2::resolve_path(cwd, path)?;
    load_inode(ino, path)
}
