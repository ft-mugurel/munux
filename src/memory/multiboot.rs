//! Multiboot2 information parsing (memory map tag).

/// EAX magic from Multiboot2-compliant loader.
pub const MULTIBOOT2_MAGIC: u32 = 0x36D7_6289;

const TAG_END: u32 = 0;
const TAG_MMAP: u32 = 6;

/// Available RAM (Multiboot2 mmap type).
pub const MMAP_AVAILABLE: u32 = 1;

#[repr(C, packed)]
struct Mb2Header {
    total_size: u32,
    reserved: u32,
}

#[repr(C, packed)]
struct Mb2Tag {
    typ: u32,
    size: u32,
}

#[repr(C, packed)]
struct MmapTagHeader {
    typ: u32,
    size: u32,
    entry_size: u32,
    entry_version: u32,
}

#[repr(C, packed)]
pub struct MmapEntry {
    pub base_addr: u64,
    pub length: u64,
    pub typ: u32,
    pub reserved: u32,
}

impl MmapEntry {
    pub fn base(&self) -> u64 {
        unsafe { core::ptr::addr_of!(self.base_addr).read_unaligned() }
    }

    pub fn length(&self) -> u64 {
        unsafe { core::ptr::addr_of!(self.length).read_unaligned() }
    }

    pub fn end(&self) -> u64 {
        self.base().saturating_add(self.length())
    }

    pub fn is_available(&self) -> bool {
        let t = unsafe { core::ptr::addr_of!(self.typ).read_unaligned() };
        t == MMAP_AVAILABLE
    }
}

/// Validate magic and walk Multiboot2 tags, calling `f` for each available mmap region.
///
/// # Safety
/// `info_addr` must point at a valid Multiboot2 info block from the bootloader.
pub unsafe fn for_each_available_region(
    magic: u32,
    info_addr: u32,
    mut f: impl FnMut(u64, u64),
) -> bool {
    if magic != MULTIBOOT2_MAGIC || info_addr == 0 {
        return false;
    }

    let hdr = &*(info_addr as *const Mb2Header);
    let total = core::ptr::addr_of!(hdr.total_size).read_unaligned() as usize;
    if total < 8 {
        return false;
    }

    let mut offset = 8usize; // skip fixed header
    let end = info_addr as usize + total;
    let mut found_mmap = false;

    while info_addr as usize + offset + 8 <= end {
        let tag_ptr = (info_addr as usize + offset) as *const Mb2Tag;
        let typ = core::ptr::addr_of!((*tag_ptr).typ).read_unaligned();
        let size = core::ptr::addr_of!((*tag_ptr).size).read_unaligned() as usize;

        if size < 8 {
            break;
        }
        if typ == TAG_END {
            break;
        }

        if typ == TAG_MMAP && size >= core::mem::size_of::<MmapTagHeader>() {
            found_mmap = true;
            let mh = tag_ptr as *const MmapTagHeader;
            let entry_size =
                core::ptr::addr_of!((*mh).entry_size).read_unaligned() as usize;
            if entry_size < core::mem::size_of::<MmapEntry>() {
                // still try minimum layout
            }
            let entries_start =
                info_addr as usize + offset + core::mem::size_of::<MmapTagHeader>();
            let entries_end = info_addr as usize + offset + size;
            let step = if entry_size >= 24 { entry_size } else { 24 };

            let mut p = entries_start;
            while p + 24 <= entries_end {
                let e = &*(p as *const MmapEntry);
                if e.is_available() && e.length() > 0 {
                    f(e.base(), e.end());
                }
                p += step;
            }
        }

        // tags are 8-byte aligned
        offset = (offset + size + 7) & !7;
    }

    found_mmap
}

/// Highest exclusive end address among available regions (for sizing the PMM).
pub unsafe fn max_available_end(magic: u32, info_addr: u32) -> Option<u64> {
    let mut max = 0u64;
    let ok = for_each_available_region(magic, info_addr, |_b, e| {
        if e > max {
            max = e;
        }
    });
    if ok && max > 0 {
        Some(max)
    } else {
        None
    }
}
