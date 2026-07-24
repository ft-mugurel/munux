//! ext2 filesystem reader (kernel-side structures + path lookup + file read).

use crate::drivers::ide;
use crate::fs::vfs::{self, FsNode, NodeType};

pub const ROOT_INODE: u32 = 2;
const EXT2_SUPER_MAGIC: u16 = 0xEF53;

// ---- On-disk structures (packed, little-endian) ----

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ext2Superblock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    pub s_r_blocks_count: u32,
    pub s_free_blocks_count: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_frag_size: u32,
    pub s_blocks_per_group: u32,
    pub s_frags_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // extended fields when rev >= 1
    pub s_first_ino: u32,
    pub s_inode_size: u16,
    pub s_block_group_nr: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ext2GroupDesc {
    pub bg_block_bitmap: u32,
    pub bg_inode_bitmap: u32,
    pub bg_inode_table: u32,
    pub bg_free_blocks_count: u16,
    pub bg_free_inodes_count: u16,
    pub bg_used_dirs_count: u16,
    pub bg_pad: u16,
    pub bg_reserved: [u32; 3],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ext2Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    pub i_block: [u32; 15],
    pub i_generation: u32,
    pub i_file_acl: u32,
    pub i_dir_acl: u32,
    pub i_faddr: u32,
    pub i_osd2: [u8; 12],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Ext2DirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    // name follows
}

// File types in directory entries (ext2 rev1)
const EXT2_FT_REG: u8 = 1;
const EXT2_FT_DIR: u8 = 2;
const EXT2_FT_CHR: u8 = 3;
const EXT2_FT_BLK: u8 = 4;
const EXT2_FT_FIFO: u8 = 5;
const EXT2_FT_SOCK: u8 = 6;
const EXT2_FT_SYMLINK: u8 = 7;

const S_IFMT: u16 = 0xF000;
const S_IFREG: u16 = 0x8000;
const S_IFDIR: u16 = 0x4000;
const S_IFLNK: u16 = 0xA000;

// ---- Kernel-side mount state ----

pub struct Ext2Fs {
    pub superblock: Ext2Superblock,
    pub block_size: u32,
    pub inode_size: u16,
    pub inodes_per_group: u32,
    pub blocks_per_group: u32,
    pub groups_count: u32,
    pub first_data_block: u32,
    pub mounted: bool,
}

static mut FS: Ext2Fs = Ext2Fs {
    superblock: unsafe { core::mem::zeroed() },
    block_size: 1024,
    inode_size: 128,
    inodes_per_group: 0,
    blocks_per_group: 0,
    groups_count: 0,
    first_data_block: 1,
    mounted: false,
};

// group descriptor cache (first groups)
const MAX_GROUPS: usize = 32;
static mut GROUPS: [Ext2GroupDesc; MAX_GROUPS] = [unsafe { core::mem::zeroed() }; MAX_GROUPS];

// ---- Block cache (LRU) ----
//
// BusyBox-sized files use single/double-indirect maps. Without a cache every
// logical block re-reads the same pointer blocks from IDE (≈3× PIO). A small
// LRU of FS blocks keeps indirect + hot data in RAM.
//
// 64 × 4 KiB = 256 KiB BSS. On 1 KiB filesystems only the first `block_size`
// bytes of each slot are used.

const BCACHE_SLOTS: usize = 64;
const BCACHE_BLOCK_CAP: usize = 4096;

#[derive(Clone, Copy)]
struct BCacheSlot {
    valid: bool,
    /// Filesystem block number when `valid`.
    block: u32,
    /// Higher = more recently used.
    tick: u64,
    data: [u8; BCACHE_BLOCK_CAP],
}

static mut BCACHE: [BCacheSlot; BCACHE_SLOTS] = [BCacheSlot {
    valid: false,
    block: 0,
    tick: 0,
    data: [0; BCACHE_BLOCK_CAP],
}; BCACHE_SLOTS];
static mut BCACHE_CLOCK: u64 = 1;
#[allow(dead_code)]
static mut BCACHE_HITS: u64 = 0;
#[allow(dead_code)]
static mut BCACHE_MISSES: u64 = 0;

fn bcache_slot_mut(i: usize) -> *mut BCacheSlot {
    unsafe { (core::ptr::addr_of_mut!(BCACHE) as *mut BCacheSlot).add(i) }
}

fn bcache_clear() {
    unsafe {
        for i in 0..BCACHE_SLOTS {
            let s = &mut *bcache_slot_mut(i);
            s.valid = false;
            s.block = 0;
            s.tick = 0;
        }
        BCACHE_CLOCK = 1;
        BCACHE_HITS = 0;
        BCACHE_MISSES = 0;
    }
}

fn bcache_next_tick() -> u64 {
    unsafe {
        BCACHE_CLOCK = BCACHE_CLOCK.wrapping_add(1);
        if BCACHE_CLOCK == 0 {
            BCACHE_CLOCK = 1;
        }
        BCACHE_CLOCK
    }
}

/// Install/overwrite a cache entry after a successful disk I/O (or write-through).
fn bcache_store(block: u32, src: &[u8], bs: usize) {
    if bs == 0 || bs > BCACHE_BLOCK_CAP {
        return;
    }
    let tick = bcache_next_tick();
    unsafe {
        // Prefer existing slot for this block, else free, else lowest tick (LRU).
        let mut target = None;
        let mut lru_i = 0usize;
        let mut lru_tick = u64::MAX;
        for i in 0..BCACHE_SLOTS {
            let s = &*bcache_slot_mut(i);
            if s.valid && s.block == block {
                target = Some(i);
                break;
            }
            if !s.valid {
                target = Some(i);
                break;
            }
            if s.tick < lru_tick {
                lru_tick = s.tick;
                lru_i = i;
            }
        }
        let i = target.unwrap_or(lru_i);
        let s = &mut *bcache_slot_mut(i);
        s.valid = true;
        s.block = block;
        s.tick = tick;
        s.data[..bs].copy_from_slice(&src[..bs]);
    }
}

/// Drop a block from the cache (e.g. after a write that bypassed write-through).
pub fn invalidate_cached_block(block: u32) {
    unsafe {
        for i in 0..BCACHE_SLOTS {
            let s = &mut *bcache_slot_mut(i);
            if s.valid && s.block == block {
                s.valid = false;
                s.block = 0;
                s.tick = 0;
            }
        }
    }
}

/// Write-through helper: keep cache coherent after a successful block write.
pub fn cache_write_block(block: u32, data: &[u8]) {
    let bs = block_size() as usize;
    if data.len() < bs || bs > BCACHE_BLOCK_CAP {
        invalidate_cached_block(block);
        return;
    }
    bcache_store(block, &data[..bs], bs);
}

unsafe fn sb_magic(sb: &Ext2Superblock) -> u16 {
    core::ptr::addr_of!(sb.s_magic).read_unaligned()
}

fn load_superblock(sb: &mut Ext2Superblock) -> Result<(), &'static str> {
    // Superblock at byte 1024 → sector 2 if 512-byte sectors
    let mut buf = [0u8; 1024];
    // Read 2 sectors starting at LBA 2
    ide::read_sector(2, &mut buf[0..512]).map_err(|_| "IDE read sb")?;
    ide::read_sector(3, &mut buf[512..1024]).map_err(|_| "IDE read sb2")?;
    unsafe {
        core::ptr::copy_nonoverlapping(
            buf.as_ptr(),
            sb as *mut Ext2Superblock as *mut u8,
            core::mem::size_of::<Ext2Superblock>(),
        );
    }
    Ok(())
}

/// Mount ext2 on the primary IDE disk.
pub fn mount() -> Result<(), &'static str> {
    let mut sb: Ext2Superblock = unsafe { core::mem::zeroed() };
    load_superblock(&mut sb)?;
    let magic = unsafe { sb_magic(&sb) };
    if magic != EXT2_SUPER_MAGIC {
        return Err("not ext2 (bad magic)");
    }

    let log_bs = unsafe { core::ptr::addr_of!(sb.s_log_block_size).read_unaligned() };
    let block_size = 1024u32 << log_bs;
    let inodes_per_group =
        unsafe { core::ptr::addr_of!(sb.s_inodes_per_group).read_unaligned() };
    let blocks_per_group =
        unsafe { core::ptr::addr_of!(sb.s_blocks_per_group).read_unaligned() };
    let blocks_count = unsafe { core::ptr::addr_of!(sb.s_blocks_count).read_unaligned() };
    let first_data_block =
        unsafe { core::ptr::addr_of!(sb.s_first_data_block).read_unaligned() };
    let rev = unsafe { core::ptr::addr_of!(sb.s_rev_level).read_unaligned() };
    let mut inode_size = 128u16;
    if rev >= 1 {
        inode_size = unsafe { core::ptr::addr_of!(sb.s_inode_size).read_unaligned() };
        if inode_size < 128 {
            inode_size = 128;
        }
    }

    let groups_count = (blocks_count + blocks_per_group - 1) / blocks_per_group;

    if block_size as usize > BCACHE_BLOCK_CAP {
        return Err("block size too large for bcache");
    }

    // Group descriptors start in the block immediately after the superblock.
    // block_size 1024: superblock in block 1 → GDT block 2
    // block_size >= 2048: superblock in block 0 → GDT block 1
    let gdt_block = if block_size == 1024 { 2u32 } else { 1u32 };

    // Mount metadata first so cached reads use the correct block size.
    bcache_clear();
    unsafe {
        FS.superblock = sb;
        FS.block_size = block_size;
        FS.inode_size = inode_size;
        FS.inodes_per_group = inodes_per_group;
        FS.blocks_per_group = blocks_per_group;
        FS.groups_count = groups_count;
        FS.first_data_block = first_data_block;
        FS.mounted = true;
    }

    let n_groups = (groups_count as usize).min(MAX_GROUPS);
    {
        let mut gbuf = [0u8; 4096];
        let blen = (block_size as usize).min(4096);
        read_block(gdt_block, block_size, &mut gbuf[..blen])?;
        let gsize = core::mem::size_of::<Ext2GroupDesc>();
        unsafe {
            for i in 0..n_groups {
                let off = i * gsize;
                if off + gsize > blen {
                    break;
                }
                core::ptr::copy_nonoverlapping(
                    gbuf.as_ptr().add(off),
                    core::ptr::addr_of_mut!(GROUPS[i]) as *mut u8,
                    gsize,
                );
            }
        }
    }

    let _ = (blocks_count, block_size, groups_count, inodes_per_group);

    // Warm cache with root directory listing
    let _ = list_dir(ROOT_INODE);
    Ok(())
}

pub fn is_mounted() -> bool {
    unsafe { FS.mounted }
}

fn block_size() -> u32 {
    unsafe { FS.block_size }
}

/// Read one filesystem block into `buf` (must be ≥ `bs`).
///
/// Uses the in-memory LRU when `bs` matches the mounted filesystem block size.
pub fn read_block(block: u32, bs: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    if buf.len() < bs as usize {
        return Err("block buffer small");
    }
    let bs_usize = bs as usize;
    if bs_usize == 0 || bs_usize > BCACHE_BLOCK_CAP || bs % 512 != 0 {
        return Err("bad block size");
    }

    // Cache path (mounted FS, normal block size).
    let use_cache = unsafe { FS.mounted && FS.block_size == bs };
    if use_cache {
        unsafe {
            for i in 0..BCACHE_SLOTS {
                let s = &mut *bcache_slot_mut(i);
                if s.valid && s.block == block {
                    buf[..bs_usize].copy_from_slice(&s.data[..bs_usize]);
                    s.tick = bcache_next_tick();
                    BCACHE_HITS = BCACHE_HITS.wrapping_add(1);
                    return Ok(());
                }
            }
            BCACHE_MISSES = BCACHE_MISSES.wrapping_add(1);
        }
    }

    let sectors_per = bs / 512;
    let lba = block * sectors_per;
    ide::read_sectors(lba, sectors_per, &mut buf[..bs_usize])?;

    if use_cache {
        bcache_store(block, &buf[..bs_usize], bs_usize);
    }
    Ok(())
}

fn read_fs_block(block: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    let bs = block_size();
    if buf.len() < bs as usize {
        return Err("block buffer small");
    }
    read_block(block, bs, &mut buf[..bs as usize])
}

/// Optional debug counters (hits / misses) for the block cache.
#[allow(dead_code)]
pub fn bcache_stats() -> (u64, u64) {
    unsafe { (BCACHE_HITS, BCACHE_MISSES) }
}

/// Read inode `ino` (1-based).
pub fn read_inode(ino: u32) -> Result<Ext2Inode, &'static str> {
    if !is_mounted() || ino == 0 {
        return Err("bad inode");
    }
    unsafe {
        let ipg = FS.inodes_per_group;
        let group = (ino - 1) / ipg;
        let index = (ino - 1) % ipg;
        if group as usize >= MAX_GROUPS {
            return Err("group OOB");
        }
        let table_block = core::ptr::addr_of!(GROUPS[group as usize].bg_inode_table).read_unaligned();
        let inode_size = FS.inode_size as u32;
        let bs = FS.block_size;
        let byte_off = index * inode_size;
        let block = table_block + byte_off / bs;
        let offset = (byte_off % bs) as usize;

        let mut bbuf = [0u8; 4096];
        let blen = (bs as usize).min(4096);
        read_fs_block(block, &mut bbuf)?;
        if offset + core::mem::size_of::<Ext2Inode>() > blen {
            return Err("inode spans");
        }
        let mut inode: Ext2Inode = core::mem::zeroed();
        core::ptr::copy_nonoverlapping(
            bbuf.as_ptr().add(offset),
            &mut inode as *mut _ as *mut u8,
            core::mem::size_of::<Ext2Inode>(),
        );
        Ok(inode)
    }
}

fn inode_mode(inode: &Ext2Inode) -> u16 {
    unsafe { core::ptr::addr_of!(inode.i_mode).read_unaligned() }
}

fn inode_size(inode: &Ext2Inode) -> u32 {
    unsafe { core::ptr::addr_of!(inode.i_size).read_unaligned() }
}

fn inode_links(inode: &Ext2Inode) -> u16 {
    unsafe { core::ptr::addr_of!(inode.i_links_count).read_unaligned() }
}

fn inode_block(inode: &Ext2Inode, i: usize) -> u32 {
    unsafe { core::ptr::addr_of!(inode.i_block[i]).read_unaligned() }
}

fn mode_to_type(mode: u16) -> NodeType {
    match mode & S_IFMT {
        S_IFREG => NodeType::Regular,
        S_IFDIR => NodeType::Directory,
        S_IFLNK => NodeType::Symlink,
        _ => NodeType::Unknown,
    }
}

fn ft_to_type(ft: u8) -> NodeType {
    match ft {
        EXT2_FT_REG => NodeType::Regular,
        EXT2_FT_DIR => NodeType::Directory,
        EXT2_FT_SYMLINK => NodeType::Symlink,
        EXT2_FT_CHR => NodeType::CharDev,
        EXT2_FT_BLK => NodeType::BlockDev,
        EXT2_FT_FIFO => NodeType::Fifo,
        EXT2_FT_SOCK => NodeType::Socket,
        _ => NodeType::Unknown,
    }
}

/// Read a little-endian u32 block pointer from filesystem block `blk` at index `idx`.
fn read_block_ptr(blk: u32, idx: usize) -> Result<u32, &'static str> {
    if blk == 0 {
        return Ok(0);
    }
    let mut bbuf = [0u8; 4096];
    read_fs_block(blk, &mut bbuf)?;
    let off = idx * 4;
    if off + 4 > bbuf.len() {
        return Err("block ptr OOB");
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&bbuf[off..off + 4]);
    Ok(u32::from_le_bytes(b))
}

/// Get data block number for file logical block `lb`.
/// Supports direct, single-indirect, and double-indirect (enough for ~1 MiB BusyBox
/// on 1 KiB blocks; triple not needed yet).
fn get_data_block(inode: &Ext2Inode, lb: u32) -> Result<u32, &'static str> {
    if lb < 12 {
        return Ok(inode_block(inode, lb as usize));
    }
    let bs = block_size();
    let per = bs / 4; // pointers per indirect block
    // single indirect: blocks 12 .. 12+per-1
    if lb < 12 + per {
        let ind = inode_block(inode, 12);
        if ind == 0 {
            return Ok(0);
        }
        return read_block_ptr(ind, (lb - 12) as usize);
    }
    // double indirect: blocks 12+per .. 12+per+per*per-1
    let dbase = 12 + per;
    let dmax = dbase + per * per;
    if lb < dmax {
        let dind = inode_block(inode, 13);
        if dind == 0 {
            return Ok(0);
        }
        let off = lb - dbase;
        let first = (off / per) as usize;
        let second = (off % per) as usize;
        let ind = read_block_ptr(dind, first)?;
        if ind == 0 {
            return Ok(0);
        }
        return read_block_ptr(ind, second);
    }
    Err("file too large (need triple indirect)")
}

/// Read up to `buf.len()` bytes from file inode at offset.
pub fn read_file(ino: u32, offset: u32, buf: &mut [u8]) -> Result<usize, &'static str> {
    let inode = read_inode(ino)?;
    let size = inode_size(&inode);
    if offset >= size {
        return Ok(0);
    }
    let to_read = core::cmp::min(buf.len() as u32, size - offset) as usize;
    let bs = block_size();
    let mut done = 0usize;
    while done < to_read {
        let pos = offset + done as u32;
        let lb = pos / bs;
        let boff = (pos % bs) as usize;
        let block = get_data_block(&inode, lb)?;
        if block == 0 {
            // sparse hole
            let chunk = core::cmp::min(to_read - done, bs as usize - boff);
            for b in buf[done..done + chunk].iter_mut() {
                *b = 0;
            }
            done += chunk;
            continue;
        }
        let mut bbuf = [0u8; 4096];
        read_fs_block(block, &mut bbuf)?;
        let chunk = core::cmp::min(to_read - done, bs as usize - boff);
        buf[done..done + chunk].copy_from_slice(&bbuf[boff..boff + chunk]);
        done += chunk;
    }
    Ok(done)
}

/// List directory inode; fills VFS cache; returns count.
pub fn list_dir(dir_ino: u32) -> Result<usize, &'static str> {
    let inode = read_inode(dir_ino)?;
    let mode = inode_mode(&inode);
    if mode & S_IFMT != S_IFDIR {
        return Err("not a directory");
    }
    vfs::cache_clear();

    let size = inode_size(&inode);
    let bs = block_size();
    let mut offset = 0u32;
    let mut prev_ino = 0u32;
    let mut count = 0usize;

    while offset < size {
        let lb = offset / bs;
        let boff = (offset % bs) as usize;
        let block = get_data_block(&inode, lb)?;
        if block == 0 {
            break;
        }
        let mut bbuf = [0u8; 4096];
        read_fs_block(block, &mut bbuf)?;

        let mut pos = boff;
        while pos + 8 <= bs as usize && offset < size {
            let ent = unsafe { &*(bbuf.as_ptr().add(pos) as *const Ext2DirEntry) };
            let ino = unsafe { core::ptr::addr_of!(ent.inode).read_unaligned() };
            let rec_len = unsafe { core::ptr::addr_of!(ent.rec_len).read_unaligned() } as usize;
            let name_len = unsafe { core::ptr::addr_of!(ent.name_len).read_unaligned() } as usize;
            let file_type = unsafe { core::ptr::addr_of!(ent.file_type).read_unaligned() };

            if rec_len < 8 {
                break;
            }
            if ino != 0 && name_len > 0 && pos + 8 + name_len <= bs as usize {
                let name_bytes = &bbuf[pos + 8..pos + 8 + name_len];
                let name = core::str::from_utf8(name_bytes).unwrap_or("?");

                let child_inode = read_inode(ino).ok();
                let mut node = FsNode::empty();
                node.used = true;
                node.set_name(name);
                node.inode = ino;
                node.father = dir_ino;
                node.master = ROOT_INODE;
                node.next_kin = 0;
                if let Some(ci) = child_inode {
                    node.size = inode_size(&ci);
                    node.links = inode_links(&ci);
                    node.rights = inode_mode(&ci) & 0x0FFF;
                    node.kind = mode_to_type(inode_mode(&ci));
                } else {
                    node.kind = ft_to_type(file_type);
                }
                // link previous sibling
                if prev_ino != 0 {
                    // patch previous next_kin in cache if present
                    for i in 0..vfs::cache_len() {
                        if let Some(mut n) = vfs::cache_get(i) {
                            if n.inode == prev_ino {
                                n.next_kin = ino;
                                // re-push not easy; store next_kin only on current chain
                            }
                        }
                    }
                }
                // set children on a synthetic parent walk via cache only
                vfs::cache_push(node);
                prev_ino = ino;
                count += 1;
            }
            pos += rec_len;
            offset += rec_len as u32;
            if rec_len == 0 {
                break;
            }
        }
        // align to next block if needed
        if offset % bs != 0 && pos >= bs as usize {
            offset = (offset / bs + 1) * bs;
        }
    }

    // Fill parent children[] from cache
    // (optional; list uses cache directly)
    Ok(count)
}

/// Linux `d_type` values for getdents64.
pub mod dt {
    pub const UNKNOWN: u8 = 0;
    pub const FIFO: u8 = 1;
    pub const CHR: u8 = 2;
    pub const DIR: u8 = 4;
    pub const BLK: u8 = 6;
    pub const REG: u8 = 8;
    pub const LNK: u8 = 10;
    pub const SOCK: u8 = 12;
}

fn ext2_ft_to_dt(ft: u8) -> u8 {
    match ft {
        EXT2_FT_REG => dt::REG,
        EXT2_FT_DIR => dt::DIR,
        EXT2_FT_CHR => dt::CHR,
        EXT2_FT_BLK => dt::BLK,
        EXT2_FT_FIFO => dt::FIFO,
        EXT2_FT_SOCK => dt::SOCK,
        EXT2_FT_SYMLINK => dt::LNK,
        _ => dt::UNKNOWN,
    }
}

/// One directory entry for getdents64 (kernel-side).
pub struct DirentInfo {
    pub ino: u32,
    pub next_off: u32,
    pub d_type: u8,
    pub name: [u8; 255],
    pub name_len: u8,
}

/// Read the next valid directory entry at or after byte `offset` in the dir.
/// Returns `Ok(None)` at end of directory.
pub fn dir_next_entry(dir_ino: u32, mut offset: u32) -> Result<Option<DirentInfo>, &'static str> {
    let inode = read_inode(dir_ino)?;
    let mode = inode_mode(&inode);
    if mode & S_IFMT != S_IFDIR {
        return Err("not a directory");
    }
    let size = inode_size(&inode);
    let bs = block_size();

    while offset < size {
        let lb = offset / bs;
        let boff = (offset % bs) as usize;
        let block = get_data_block(&inode, lb)?;
        if block == 0 {
            break;
        }
        let mut bbuf = [0u8; 4096];
        read_fs_block(block, &mut bbuf)?;

        let mut pos = boff;
        while pos + 8 <= bs as usize {
            let ent = unsafe { &*(bbuf.as_ptr().add(pos) as *const Ext2DirEntry) };
            let ino = unsafe { core::ptr::addr_of!(ent.inode).read_unaligned() };
            let rec_len = unsafe { core::ptr::addr_of!(ent.rec_len).read_unaligned() } as usize;
            let name_len = unsafe { core::ptr::addr_of!(ent.name_len).read_unaligned() } as usize;
            let file_type = unsafe { core::ptr::addr_of!(ent.file_type).read_unaligned() };

            if rec_len < 8 {
                return Ok(None);
            }
            let entry_off = (lb * bs) + pos as u32;
            let next_off = entry_off + rec_len as u32;

            if ino != 0 && name_len > 0 && pos + 8 + name_len <= bs as usize && entry_off >= offset
            {
                let name_bytes = &bbuf[pos + 8..pos + 8 + name_len];
                let mut info = DirentInfo {
                    ino,
                    next_off,
                    d_type: ext2_ft_to_dt(file_type),
                    name: [0; 255],
                    name_len: name_len.min(255) as u8,
                };
                let n = info.name_len as usize;
                info.name[..n].copy_from_slice(&name_bytes[..n]);
                // If type unknown, try inode mode
                if info.d_type == dt::UNKNOWN {
                    if let Ok(ci) = read_inode(ino) {
                        let m = inode_mode(&ci);
                        info.d_type = if m & S_IFMT == S_IFDIR {
                            dt::DIR
                        } else if m & S_IFMT == S_IFREG {
                            dt::REG
                        } else if m & S_IFMT == S_IFLNK {
                            dt::LNK
                        } else {
                            dt::UNKNOWN
                        };
                    }
                }
                return Ok(Some(info));
            }

            pos += rec_len;
            if rec_len == 0 {
                break;
            }
            // advance search offset past this record
            offset = next_off;
        }
        // next block
        offset = (lb + 1) * bs;
    }
    Ok(None)
}

/// Lookup a single path component in directory.
pub fn lookup(dir_ino: u32, name: &str) -> Result<u32, &'static str> {
    list_dir(dir_ino)?;
    for i in 0..vfs::cache_len() {
        if let Some(n) = vfs::cache_get(i) {
            if n.name_str() == name {
                return Ok(n.inode);
            }
        }
    }
    Err("not found")
}

/// Resolve path relative to `cwd_ino`. Absolute if starts with '/'.
pub fn resolve_path(cwd_ino: u32, path: &str) -> Result<u32, &'static str> {
    if path.is_empty() {
        return Ok(cwd_ino);
    }
    let mut ino = if path.starts_with('/') {
        ROOT_INODE
    } else {
        cwd_ino
    };
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            // look up .. in directory
            ino = lookup(ino, "..").unwrap_or(ROOT_INODE);
            continue;
        }
        ino = lookup(ino, part)?;
    }
    Ok(ino)
}

pub fn inode_is_dir(ino: u32) -> bool {
    read_inode(ino)
        .map(|i| inode_mode(&i) & S_IFMT == S_IFDIR)
        .unwrap_or(false)
}

pub fn inode_file_size(ino: u32) -> u32 {
    read_inode(ino).map(|i| inode_size(&i)).unwrap_or(0)
}

/// Fields needed to fill Linux `struct stat` (x86_64).
#[derive(Clone, Copy)]
pub struct InodeStat {
    pub mode: u16,
    pub uid: u16,
    pub gid: u16,
    pub nlink: u16,
    pub size: u32,
    /// ext2 `i_blocks` is in 512-byte units already on disk.
    pub blocks_512: u32,
    pub atime: u32,
    pub mtime: u32,
    pub ctime: u32,
}

pub fn inode_stat(ino: u32) -> Result<InodeStat, &'static str> {
    let i = read_inode(ino)?;
    Ok(InodeStat {
        mode: inode_mode(&i),
        uid: unsafe { core::ptr::addr_of!(i.i_uid).read_unaligned() },
        gid: unsafe { core::ptr::addr_of!(i.i_gid).read_unaligned() },
        nlink: inode_links(&i),
        size: inode_size(&i),
        blocks_512: unsafe { core::ptr::addr_of!(i.i_blocks).read_unaligned() },
        atime: unsafe { core::ptr::addr_of!(i.i_atime).read_unaligned() },
        mtime: unsafe { core::ptr::addr_of!(i.i_mtime).read_unaligned() },
        ctime: unsafe { core::ptr::addr_of!(i.i_ctime).read_unaligned() },
    })
}

// ---- Accessors for write module (same crate) ----

pub unsafe fn fs_superblock_ptr() -> *mut Ext2Superblock {
    core::ptr::addr_of_mut!(FS.superblock)
}

pub unsafe fn fs_group_ptr(i: usize) -> *mut Ext2GroupDesc {
    core::ptr::addr_of_mut!(GROUPS[i])
}

pub unsafe fn fs_block_size() -> u32 {
    FS.block_size
}

pub unsafe fn fs_inode_size() -> u16 {
    FS.inode_size
}

pub unsafe fn fs_inodes_per_group() -> u32 {
    FS.inodes_per_group
}

pub unsafe fn fs_blocks_per_group() -> u32 {
    FS.blocks_per_group
}

pub unsafe fn fs_groups_count() -> u32 {
    FS.groups_count
}

pub unsafe fn fs_first_data_block() -> u32 {
    FS.first_data_block
}
