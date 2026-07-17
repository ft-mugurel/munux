//! Multiboot 1 information structure parsing (memory map).

/// Multiboot magic value placed in EAX by a compliant bootloader.
pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BAD_B002;

/// `flags` bit 0: mem_lower / mem_upper present.
const FLAG_MEM: u32 = 1 << 0;
/// `flags` bit 6: mmap_addr / mmap_length present.
const FLAG_MMAP: u32 = 1 << 6;

/// Multiboot mmap type: available RAM.
pub const MMAP_AVAILABLE: u32 = 1;

/// Multiboot information structure (fields we care about).
/// Layout matches the Multiboot 1 spec (partial).
#[repr(C, packed)]
pub struct MultibootInfo {
    pub flags: u32,
    pub mem_lower: u32, // KiB of memory below 1 MiB
    pub mem_upper: u32, // KiB of memory above 1 MiB
    pub boot_device: u32,
    pub cmdline: u32,
    pub mods_count: u32,
    pub mods_addr: u32,
    pub syms: [u32; 4],
    pub mmap_length: u32,
    pub mmap_addr: u32,
}

/// One memory-map entry. `size` is the size of the *rest* of the entry.
#[repr(C, packed)]
pub struct MmapEntry {
    pub size: u32,
    pub base_addr_low: u32,
    pub base_addr_high: u32,
    pub length_low: u32,
    pub length_high: u32,
    pub type_: u32,
}

impl MmapEntry {
    pub fn base(&self) -> u64 {
        let lo = unsafe { core::ptr::addr_of!(self.base_addr_low).read_unaligned() };
        let hi = unsafe { core::ptr::addr_of!(self.base_addr_high).read_unaligned() };
        (hi as u64) << 32 | lo as u64
    }

    pub fn length(&self) -> u64 {
        let lo = unsafe { core::ptr::addr_of!(self.length_low).read_unaligned() };
        let hi = unsafe { core::ptr::addr_of!(self.length_high).read_unaligned() };
        (hi as u64) << 32 | lo as u64
    }

    pub fn end(&self) -> u64 {
        self.base().saturating_add(self.length())
    }

    pub fn is_available(&self) -> bool {
        let t = unsafe { core::ptr::addr_of!(self.type_).read_unaligned() };
        t == MMAP_AVAILABLE
    }

    pub fn entry_size(&self) -> u32 {
        unsafe { core::ptr::addr_of!(self.size).read_unaligned() }
    }
}

/// Validate magic and return a reference to the Multiboot info struct.
///
/// # Safety
/// `info_addr` must point at a valid Multiboot info structure from the bootloader.
pub unsafe fn load(magic: u32, info_addr: u32) -> Option<&'static MultibootInfo> {
    if magic != MULTIBOOT_BOOTLOADER_MAGIC {
        return None;
    }
    if info_addr == 0 {
        return None;
    }
    Some(&*(info_addr as *const MultibootInfo))
}

impl MultibootInfo {
    pub fn has_mem(&self) -> bool {
        let flags = unsafe { core::ptr::addr_of!(self.flags).read_unaligned() };
        flags & FLAG_MEM != 0
    }

    pub fn has_mmap(&self) -> bool {
        let flags = unsafe { core::ptr::addr_of!(self.flags).read_unaligned() };
        flags & FLAG_MMAP != 0
    }

    pub fn mem_upper_kib(&self) -> u32 {
        unsafe { core::ptr::addr_of!(self.mem_upper).read_unaligned() }
    }

    pub fn mmap_addr(&self) -> u32 {
        unsafe { core::ptr::addr_of!(self.mmap_addr).read_unaligned() }
    }

    pub fn mmap_length(&self) -> u32 {
        unsafe { core::ptr::addr_of!(self.mmap_length).read_unaligned() }
    }

    /// Iterate mmap entries if present.
    pub fn mmap_entries(&self) -> MmapIter {
        let addr = self.mmap_addr();
        let len = self.mmap_length();
        if !self.has_mmap() || addr == 0 || len == 0 {
            return MmapIter {
                cursor: 0,
                end: 0,
            };
        }
        MmapIter {
            cursor: addr as usize,
            end: addr as usize + len as usize,
        }
    }
}

pub struct MmapIter {
    cursor: usize,
    end: usize,
}

impl Iterator for MmapIter {
    type Item = &'static MmapEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor + 4 > self.end {
            return None;
        }
        let entry = unsafe { &*(self.cursor as *const MmapEntry) };
        let size = entry.entry_size() as usize;
        // size field describes remaining bytes; total entry = size + 4
        let total = size.checked_add(4)?;
        if size < 20 {
            // minimum: base(8)+len(8)+type(4) after size
            return None;
        }
        self.cursor = self.cursor.saturating_add(total);
        Some(entry)
    }
}
