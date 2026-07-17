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

    // Group descriptors start in the block immediately after the superblock.
    // block_size 1024: superblock in block 1 → GDT block 2
    // block_size >= 2048: superblock in block 0 → GDT block 1
    let gdt_block = if block_size == 1024 { 2u32 } else { 1u32 };

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

    crate::println!(
        "ext2: blocks={} block_size={} groups={} inodes/group={}",
        blocks_count,
        block_size,
        groups_count,
        inodes_per_group
    );

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

/// Read one filesystem block into buf (len >= block_size).
pub fn read_block(block: u32, bs: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    let sectors_per = bs / 512;
    let lba = block * sectors_per;
    ide::read_sectors(lba, sectors_per, buf)
}

fn read_fs_block(block: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    let bs = block_size();
    if buf.len() < bs as usize {
        return Err("block buffer small");
    }
    read_block(block, bs, &mut buf[..bs as usize])
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

/// Get data block number for file logical block `lb` (direct + single indirect only).
fn get_data_block(inode: &Ext2Inode, lb: u32) -> Result<u32, &'static str> {
    if lb < 12 {
        return Ok(inode_block(inode, lb as usize));
    }
    // single indirect
    let bs = block_size();
    let per = bs / 4;
    if lb < 12 + per {
        let ind = inode_block(inode, 12);
        if ind == 0 {
            return Ok(0);
        }
        let mut bbuf = [0u8; 4096];
        read_fs_block(ind, &mut bbuf)?;
        let idx = (lb - 12) as usize;
        let off = idx * 4;
        let mut b = [0u8; 4];
        b.copy_from_slice(&bbuf[off..off + 4]);
        return Ok(u32::from_le_bytes(b));
    }
    Err("file too large (need double indirect)")
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
