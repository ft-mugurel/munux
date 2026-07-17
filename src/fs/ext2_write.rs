//! ext2 write support: mkdir, touch/create, unlink (rm), rmdir.

use crate::drivers::ide;
use crate::fs::ext2::{
    self, read_block, read_inode, Ext2GroupDesc, Ext2Inode, Ext2Superblock, ROOT_INODE,
};

const S_IFREG: u16 = 0x8000;
const S_IFDIR: u16 = 0x4000;
const S_IFMT: u16 = 0xF000;

const EXT2_FT_REG: u8 = 1;
const EXT2_FT_DIR: u8 = 2;

const MAX_GROUPS: usize = 32;

// Re-access mount state through helpers in ext2 module — we use public read/write
// and duplicate minimal FS state access via re-exported internals.

// Access to static FS/GROUPS: implemented by functions in ext2 that we call,
// plus local write helpers that use the same layout.

fn fs_block_size() -> u32 {
    unsafe { crate::fs::ext2::fs_block_size() }
}

fn fs_inode_size() -> u16 {
    unsafe { crate::fs::ext2::fs_inode_size() }
}

fn fs_inodes_per_group() -> u32 {
    unsafe { crate::fs::ext2::fs_inodes_per_group() }
}

fn fs_blocks_per_group() -> u32 {
    unsafe { crate::fs::ext2::fs_blocks_per_group() }
}

fn fs_groups_count() -> u32 {
    unsafe { crate::fs::ext2::fs_groups_count() }
}

fn fs_first_data_block() -> u32 {
    unsafe { crate::fs::ext2::fs_first_data_block() }
}

fn gdt_block() -> u32 {
    if fs_block_size() == 1024 {
        2
    } else {
        1
    }
}

fn write_fs_block(block: u32, buf: &[u8]) -> Result<(), &'static str> {
    let bs = fs_block_size();
    if buf.len() < bs as usize {
        return Err("write buffer small");
    }
    let sectors = bs / 512;
    let lba = block * sectors;
    ide::write_sectors(lba, sectors, &buf[..bs as usize]).map_err(|_| "IDE write block")
}

fn read_fs_block(block: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    let bs = fs_block_size();
    if buf.len() < bs as usize {
        return Err("read buffer small");
    }
    read_block(block, bs, &mut buf[..bs as usize])
}

fn write_superblock() -> Result<(), &'static str> {
    // Superblock lives at byte offset 1024 (sectors 2..3 for first 1024 bytes of SB area)
    let mut buf = [0u8; 1024];
    ide::read_sector(2, &mut buf[0..512]).map_err(|_| "IDE r")?;
    ide::read_sector(3, &mut buf[512..1024]).map_err(|_| "IDE r")?;
    unsafe {
        let sb = crate::fs::ext2::fs_superblock_ptr();
        let n = core::mem::size_of::<Ext2Superblock>();
        core::ptr::copy_nonoverlapping(sb as *const u8, buf.as_mut_ptr(), n.min(1024));
    }
    ide::write_sector(2, &buf[0..512]).map_err(|_| "IDE w sb")?;
    ide::write_sector(3, &buf[512..1024]).map_err(|_| "IDE w sb2")?;
    Ok(())
}

fn write_group_desc(group: u32) -> Result<(), &'static str> {
    if group as usize >= MAX_GROUPS {
        return Err("group OOB");
    }
    let bs = fs_block_size();
    let mut gbuf = [0u8; 4096];
    let blen = (bs as usize).min(4096);
    read_fs_block(gdt_block(), &mut gbuf)?;
    let gsize = core::mem::size_of::<Ext2GroupDesc>();
    let off = (group as usize) * gsize;
    if off + gsize > blen {
        return Err("gd out of block");
    }
    unsafe {
        let gd = crate::fs::ext2::fs_group_ptr(group as usize);
        core::ptr::copy_nonoverlapping(gd as *const u8, gbuf.as_mut_ptr().add(off), gsize);
    }
    write_fs_block(gdt_block(), &gbuf)
}

fn bitmap_test(bm: &[u8], bit: u32) -> bool {
    let b = (bit / 8) as usize;
    let m = 1u8 << (bit % 8);
    bm.get(b).map(|x| x & m != 0).unwrap_or(true)
}

fn bitmap_set(bm: &mut [u8], bit: u32, used: bool) {
    let b = (bit / 8) as usize;
    let m = 1u8 << (bit % 8);
    if b >= bm.len() {
        return;
    }
    if used {
        bm[b] |= m;
    } else {
        bm[b] &= !m;
    }
}

fn group_first_block(group: u32) -> u32 {
    fs_first_data_block() + group * fs_blocks_per_group()
}

/// Allocate one free data block; returns block number.
pub fn alloc_block() -> Result<u32, &'static str> {
    let bpg = fs_blocks_per_group();
    let groups = fs_groups_count().min(MAX_GROUPS as u32);
    let mut bm = [0u8; 4096];

    for g in 0..groups {
        let bmp_block = unsafe {
            core::ptr::addr_of!((*crate::fs::ext2::fs_group_ptr(g as usize)).bg_block_bitmap)
                .read_unaligned()
        };
        read_fs_block(bmp_block, &mut bm)?;
        for bit in 0..bpg {
            if !bitmap_test(&bm, bit) {
                bitmap_set(&mut bm, bit, true);
                write_fs_block(bmp_block, &bm)?;
                // update group free count
                unsafe {
                    let gd = crate::fs::ext2::fs_group_ptr(g as usize);
                    let free =
                        core::ptr::addr_of!((*gd).bg_free_blocks_count).read_unaligned();
                    if free > 0 {
                        core::ptr::addr_of_mut!((*gd).bg_free_blocks_count)
                            .write_unaligned(free - 1);
                    }
                    let sb = crate::fs::ext2::fs_superblock_ptr();
                    let sfree =
                        core::ptr::addr_of!((*sb).s_free_blocks_count).read_unaligned();
                    if sfree > 0 {
                        core::ptr::addr_of_mut!((*sb).s_free_blocks_count)
                            .write_unaligned(sfree - 1);
                    }
                }
                write_group_desc(g)?;
                write_superblock()?;
                let blk = group_first_block(g) + bit;
                // zero the new block
                let z = [0u8; 4096];
                write_fs_block(blk, &z)?;
                return Ok(blk);
            }
        }
    }
    Err("no free blocks")
}

fn free_block(block: u32) -> Result<(), &'static str> {
    if block < fs_first_data_block() {
        return Err("bad block free");
    }
    let bpg = fs_blocks_per_group();
    let rel = block - fs_first_data_block();
    let g = rel / bpg;
    let bit = rel % bpg;
    if g as usize >= MAX_GROUPS {
        return Err("group OOB");
    }
    let mut bm = [0u8; 4096];
    let bmp_block = unsafe {
        core::ptr::addr_of!((*crate::fs::ext2::fs_group_ptr(g as usize)).bg_block_bitmap)
            .read_unaligned()
    };
    read_fs_block(bmp_block, &mut bm)?;
    if !bitmap_test(&bm, bit) {
        return Ok(()); // already free
    }
    bitmap_set(&mut bm, bit, false);
    write_fs_block(bmp_block, &bm)?;
    unsafe {
        let gd = crate::fs::ext2::fs_group_ptr(g as usize);
        let free = core::ptr::addr_of!((*gd).bg_free_blocks_count).read_unaligned();
        core::ptr::addr_of_mut!((*gd).bg_free_blocks_count).write_unaligned(free + 1);
        let sb = crate::fs::ext2::fs_superblock_ptr();
        let sfree = core::ptr::addr_of!((*sb).s_free_blocks_count).read_unaligned();
        core::ptr::addr_of_mut!((*sb).s_free_blocks_count).write_unaligned(sfree + 1);
    }
    write_group_desc(g)?;
    write_superblock()
}

/// Allocate free inode number.
pub fn alloc_inode() -> Result<u32, &'static str> {
    let ipg = fs_inodes_per_group();
    let groups = fs_groups_count().min(MAX_GROUPS as u32);
    let mut bm = [0u8; 4096];

    for g in 0..groups {
        let bmp_block = unsafe {
            core::ptr::addr_of!((*crate::fs::ext2::fs_group_ptr(g as usize)).bg_inode_bitmap)
                .read_unaligned()
        };
        read_fs_block(bmp_block, &mut bm)?;
        // inode bits: bit 0 = first inode of group; inodes start at 1
        let start_bit = if g == 0 { 1u32 } else { 0u32 }; // skip inode 0; also root etc. may be set
        for bit in start_bit..ipg {
            if !bitmap_test(&bm, bit) {
                bitmap_set(&mut bm, bit, true);
                write_fs_block(bmp_block, &bm)?;
                unsafe {
                    let gd = crate::fs::ext2::fs_group_ptr(g as usize);
                    let free =
                        core::ptr::addr_of!((*gd).bg_free_inodes_count).read_unaligned();
                    if free > 0 {
                        core::ptr::addr_of_mut!((*gd).bg_free_inodes_count)
                            .write_unaligned(free - 1);
                    }
                    let sb = crate::fs::ext2::fs_superblock_ptr();
                    let sfree =
                        core::ptr::addr_of!((*sb).s_free_inodes_count).read_unaligned();
                    if sfree > 0 {
                        core::ptr::addr_of_mut!((*sb).s_free_inodes_count)
                            .write_unaligned(sfree - 1);
                    }
                }
                write_group_desc(g)?;
                write_superblock()?;
                let ino = g * ipg + bit + 1;
                return Ok(ino);
            }
        }
    }
    Err("no free inodes")
}

fn free_inode(ino: u32) -> Result<(), &'static str> {
    if ino < 1 {
        return Err("bad ino");
    }
    let ipg = fs_inodes_per_group();
    let g = (ino - 1) / ipg;
    let bit = (ino - 1) % ipg;
    if g as usize >= MAX_GROUPS {
        return Err("group OOB");
    }
    let mut bm = [0u8; 4096];
    let bmp_block = unsafe {
        core::ptr::addr_of!((*crate::fs::ext2::fs_group_ptr(g as usize)).bg_inode_bitmap)
            .read_unaligned()
    };
    read_fs_block(bmp_block, &mut bm)?;
    bitmap_set(&mut bm, bit, false);
    write_fs_block(bmp_block, &bm)?;
    unsafe {
        let gd = crate::fs::ext2::fs_group_ptr(g as usize);
        let free = core::ptr::addr_of!((*gd).bg_free_inodes_count).read_unaligned();
        core::ptr::addr_of_mut!((*gd).bg_free_inodes_count).write_unaligned(free + 1);
        let sb = crate::fs::ext2::fs_superblock_ptr();
        let sfree = core::ptr::addr_of!((*sb).s_free_inodes_count).read_unaligned();
        core::ptr::addr_of_mut!((*sb).s_free_inodes_count).write_unaligned(sfree + 1);
    }
    write_group_desc(g)?;
    write_superblock()
}

fn write_inode(ino: u32, inode: &Ext2Inode) -> Result<(), &'static str> {
    if ino == 0 {
        return Err("bad ino");
    }
    let ipg = fs_inodes_per_group();
    let group = (ino - 1) / ipg;
    let index = (ino - 1) % ipg;
    if group as usize >= MAX_GROUPS {
        return Err("group OOB");
    }
    let table_block = unsafe {
        core::ptr::addr_of!((*crate::fs::ext2::fs_group_ptr(group as usize)).bg_inode_table)
            .read_unaligned()
    };
    let inode_size = fs_inode_size() as u32;
    let bs = fs_block_size();
    let byte_off = index * inode_size;
    let block = table_block + byte_off / bs;
    let offset = (byte_off % bs) as usize;
    let mut bbuf = [0u8; 4096];
    read_fs_block(block, &mut bbuf)?;
    let n = core::mem::size_of::<Ext2Inode>().min(inode_size as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(
            inode as *const _ as *const u8,
            bbuf.as_mut_ptr().add(offset),
            n,
        );
    }
    write_fs_block(block, &bbuf)
}

fn zero_inode() -> Ext2Inode {
    unsafe { core::mem::zeroed() }
}

fn inode_set_mode(inode: &mut Ext2Inode, mode: u16) {
    unsafe {
        core::ptr::addr_of_mut!(inode.i_mode).write_unaligned(mode);
    }
}
fn inode_set_uid(inode: &mut Ext2Inode, uid: u16) {
    unsafe {
        core::ptr::addr_of_mut!(inode.i_uid).write_unaligned(uid);
    }
}
fn inode_set_size(inode: &mut Ext2Inode, size: u32) {
    unsafe {
        core::ptr::addr_of_mut!(inode.i_size).write_unaligned(size);
    }
}
fn inode_set_links(inode: &mut Ext2Inode, n: u16) {
    unsafe {
        core::ptr::addr_of_mut!(inode.i_links_count).write_unaligned(n);
    }
}
fn inode_set_blocks(inode: &mut Ext2Inode, n: u32) {
    unsafe {
        core::ptr::addr_of_mut!(inode.i_blocks).write_unaligned(n);
    }
}
fn inode_set_block(inode: &mut Ext2Inode, i: usize, b: u32) {
    unsafe {
        core::ptr::addr_of_mut!(inode.i_block[i]).write_unaligned(b);
    }
}
fn inode_set_times(inode: &mut Ext2Inode, t: u32) {
    unsafe {
        core::ptr::addr_of_mut!(inode.i_atime).write_unaligned(t);
        core::ptr::addr_of_mut!(inode.i_ctime).write_unaligned(t);
        core::ptr::addr_of_mut!(inode.i_mtime).write_unaligned(t);
    }
}

fn inode_get_mode(inode: &Ext2Inode) -> u16 {
    unsafe { core::ptr::addr_of!(inode.i_mode).read_unaligned() }
}
fn inode_get_size(inode: &Ext2Inode) -> u32 {
    unsafe { core::ptr::addr_of!(inode.i_size).read_unaligned() }
}
fn inode_get_links(inode: &Ext2Inode) -> u16 {
    unsafe { core::ptr::addr_of!(inode.i_links_count).read_unaligned() }
}
fn inode_get_block(inode: &Ext2Inode, i: usize) -> u32 {
    unsafe { core::ptr::addr_of!(inode.i_block[i]).read_unaligned() }
}

fn now() -> u32 {
    // No RTC: use a fixed epoch-ish value (or tick counter if available)
    1_700_000_000u32.wrapping_add(crate::interrupts::timer::ticks())
}

fn dirent_rec_len(name_len: usize) -> usize {
    let n = 8 + name_len;
    (n + 3) & !3 // align 4
}

fn read_u32_le(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn read_u16_le(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

/// Add directory entry `name` → `child_ino` into directory `dir_ino`.
fn dir_add_entry(dir_ino: u32, name: &str, child_ino: u32, file_type: u8) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 255 {
        return Err("bad name");
    }
    let mut inode = read_inode(dir_ino)?;
    if inode_get_mode(&inode) & S_IFMT != S_IFDIR {
        return Err("not a directory");
    }
    let bs = fs_block_size() as usize;
    let need = dirent_rec_len(name.len());
    let size = inode_get_size(&inode);
    let mut offset = 0u32;

    // Scan for free space in existing entries
    while offset < size {
        let lb = offset / bs as u32;
        let boff = (offset % bs as u32) as usize;
        let block = {
            if lb < 12 {
                inode_get_block(&inode, lb as usize)
            } else {
                return Err("dir too large");
            }
        };
        if block == 0 {
            break;
        }
        let mut bbuf = [0u8; 4096];
        read_fs_block(block, &mut bbuf)?;
        let mut pos = boff;
        while pos + 8 <= bs && (offset as usize) < size as usize {
            let ino = read_u32_le(&bbuf, pos);
            let rec = read_u16_le(&bbuf, pos + 4) as usize;
            let nlen = bbuf[pos + 6] as usize;
            if rec < 8 {
                break;
            }
            let ideal = dirent_rec_len(nlen);
            // empty slot
            if ino == 0 && rec >= need {
                write_dirent(&mut bbuf, pos, rec, child_ino, name, file_type);
                write_fs_block(block, &bbuf)?;
                return Ok(());
            }
            // split trailing free space in this entry
            if ino != 0 && rec >= ideal + need {
                let new_rec = ideal;
                let rest = rec - ideal;
                let nr = (new_rec as u16).to_le_bytes();
                bbuf[pos + 4] = nr[0];
                bbuf[pos + 5] = nr[1];
                let npos = pos + new_rec;
                write_dirent(&mut bbuf, npos, rest, child_ino, name, file_type);
                write_fs_block(block, &bbuf)?;
                return Ok(());
            }
            pos += rec;
            offset += rec as u32;
            if rec == 0 {
                break;
            }
        }
        if offset % bs as u32 != 0 {
            offset = (offset / bs as u32 + 1) * bs as u32;
        }
    }

    // Need a new directory block
    let new_blk = alloc_block()?;
    let mut bbuf = [0u8; 4096];
    write_dirent(&mut bbuf, 0, bs, child_ino, name, file_type);
    // rest of block is empty (rec_len already covers full block for first entry)
    write_fs_block(new_blk, &bbuf)?;

    // attach to inode
    let mut placed = false;
    for i in 0..12 {
        if inode_get_block(&inode, i) == 0 {
            inode_set_block(&mut inode, i, new_blk);
            placed = true;
            break;
        }
    }
    if !placed {
        return Err("no direct block slot");
    }
    let new_size = size + bs as u32;
    inode_set_size(&mut inode, new_size);
    // i_blocks in 512-byte units
    let ib = unsafe { core::ptr::addr_of!(inode.i_blocks).read_unaligned() } + (bs as u32 / 512);
    inode_set_blocks(&mut inode, ib);
    inode_set_times(&mut inode, now());
    write_inode(dir_ino, &inode)
}

fn write_dirent(buf: &mut [u8], pos: usize, rec_len: usize, ino: u32, name: &str, ftype: u8) {
    let nlen = name.len();
    buf[pos..pos + 4].copy_from_slice(&ino.to_le_bytes());
    buf[pos + 4..pos + 6].copy_from_slice(&(rec_len as u16).to_le_bytes());
    buf[pos + 6] = nlen as u8;
    buf[pos + 7] = ftype;
    buf[pos + 8..pos + 8 + nlen].copy_from_slice(name.as_bytes());
}

fn dir_remove_entry(dir_ino: u32, name: &str) -> Result<u32, &'static str> {
    // returns removed child inode
    let inode = read_inode(dir_ino)?;
    if inode_get_mode(&inode) & S_IFMT != S_IFDIR {
        return Err("not a directory");
    }
    let bs = fs_block_size() as usize;
    let size = inode_get_size(&inode);
    let mut offset = 0u32;

    while offset < size {
        let lb = offset / bs as u32;
        let block = inode_get_block(&inode, lb as usize);
        if block == 0 {
            break;
        }
        let mut bbuf = [0u8; 4096];
        read_fs_block(block, &mut bbuf)?;
        let mut pos = 0usize;
        let mut prev_pos: Option<usize> = None;
        while pos + 8 <= bs {
            let ino = read_u32_le(&bbuf, pos);
            let rec = read_u16_le(&bbuf, pos + 4) as usize;
            let nlen = bbuf[pos + 6] as usize;
            if rec < 8 {
                break;
            }
            if ino != 0 && nlen > 0 && pos + 8 + nlen <= bs {
                let nm = core::str::from_utf8(&bbuf[pos + 8..pos + 8 + nlen]).unwrap_or("");
                if nm == name {
                    if nm == "." || nm == ".." {
                        return Err("cannot remove . or ..");
                    }
                    // merge into previous or zero inode
                    if let Some(pp) = prev_pos {
                        let prec = read_u16_le(&bbuf, pp + 4) as usize;
                        let new_rec = (prec + rec) as u16;
                        bbuf[pp + 4..pp + 6].copy_from_slice(&new_rec.to_le_bytes());
                    } else {
                        // first entry: zero inode, keep rec_len
                        bbuf[pos..pos + 4].copy_from_slice(&0u32.to_le_bytes());
                    }
                    write_fs_block(block, &bbuf)?;
                    return Ok(ino);
                }
            }
            if ino != 0 {
                prev_pos = Some(pos);
            }
            pos += rec;
            offset += rec as u32;
            if rec == 0 {
                break;
            }
        }
        if offset % bs as u32 != 0 {
            offset = (offset / bs as u32 + 1) * bs as u32;
        }
    }
    Err("not found")
}

fn split_parent_name(cwd: u32, path: &str) -> Result<(u32, &str), &'static str> {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return Err("invalid path");
    }
    if let Some(pos) = path.rfind('/') {
        let (dir, name) = path.split_at(pos);
        let name = &name[1..];
        if name.is_empty() {
            return Err("invalid name");
        }
        let parent = if dir.is_empty() {
            ROOT_INODE
        } else {
            ext2::resolve_path(cwd, dir)?
        };
        Ok((parent, name))
    } else {
        Ok((cwd, path))
    }
}

/// Create an empty regular file (touch). If exists, update mtime only.
pub fn touch(cwd: u32, path: &str) -> Result<u32, &'static str> {
    if !ext2::is_mounted() {
        return Err("not mounted");
    }
    let (parent, name) = split_parent_name(cwd, path)?;
    if name == "." || name == ".." {
        return Err("bad name");
    }
    if let Ok(existing) = ext2::lookup(parent, name) {
        // update times
        let mut ino = read_inode(existing)?;
        inode_set_times(&mut ino, now());
        write_inode(existing, &ino)?;
        return Ok(existing);
    }

    let ino_num = alloc_inode()?;
    let mut inode = zero_inode();
    inode_set_mode(&mut inode, S_IFREG | 0o644);
    inode_set_uid(&mut inode, 0);
    inode_set_size(&mut inode, 0);
    inode_set_links(&mut inode, 1);
    inode_set_blocks(&mut inode, 0);
    inode_set_times(&mut inode, now());
    write_inode(ino_num, &inode)?;
    dir_add_entry(parent, name, ino_num, EXT2_FT_REG)?;
    Ok(ino_num)
}

/// Create a directory.
pub fn mkdir(cwd: u32, path: &str) -> Result<u32, &'static str> {
    if !ext2::is_mounted() {
        return Err("not mounted");
    }
    let (parent, name) = split_parent_name(cwd, path)?;
    if name == "." || name == ".." {
        return Err("bad name");
    }
    if ext2::lookup(parent, name).is_ok() {
        return Err("exists");
    }

    let ino_num = alloc_inode()?;
    let blk = alloc_block()?;
    let bs = fs_block_size() as usize;

    // directory data: . and ..
    let mut bbuf = [0u8; 4096];
    let rec_dot = dirent_rec_len(1);
    write_dirent(&mut bbuf, 0, rec_dot, ino_num, ".", EXT2_FT_DIR);
    write_dirent(
        &mut bbuf,
        rec_dot,
        bs - rec_dot,
        parent,
        "..",
        EXT2_FT_DIR,
    );
    write_fs_block(blk, &bbuf)?;

    let mut inode = zero_inode();
    inode_set_mode(&mut inode, S_IFDIR | 0o755);
    inode_set_uid(&mut inode, 0);
    inode_set_size(&mut inode, bs as u32);
    inode_set_links(&mut inode, 2); // . and parent entry
    inode_set_blocks(&mut inode, (bs as u32) / 512);
    inode_set_block(&mut inode, 0, blk);
    inode_set_times(&mut inode, now());
    write_inode(ino_num, &inode)?;

    dir_add_entry(parent, name, ino_num, EXT2_FT_DIR)?;

    // parent links++
    let mut pino = read_inode(parent)?;
    let links = inode_get_links(&pino) + 1;
    inode_set_links(&mut pino, links);
    inode_set_times(&mut pino, now());
    write_inode(parent, &pino)?;

    // group used_dirs++
    let ipg = fs_inodes_per_group();
    let g = (ino_num - 1) / ipg;
    if (g as usize) < MAX_GROUPS {
        unsafe {
            let gd = crate::fs::ext2::fs_group_ptr(g as usize);
            let d = core::ptr::addr_of!((*gd).bg_used_dirs_count).read_unaligned();
            core::ptr::addr_of_mut!((*gd).bg_used_dirs_count).write_unaligned(d + 1);
        }
        write_group_desc(g)?;
    }
    Ok(ino_num)
}

/// Remove a regular file.
pub fn unlink(cwd: u32, path: &str) -> Result<(), &'static str> {
    if !ext2::is_mounted() {
        return Err("not mounted");
    }
    let (parent, name) = split_parent_name(cwd, path)?;
    if name == "." || name == ".." {
        return Err("bad name");
    }
    let child = ext2::lookup(parent, name)?;
    let mut inode = read_inode(child)?;
    if inode_get_mode(&inode) & S_IFMT == S_IFDIR {
        return Err("is a directory (use rmdir)");
    }
    let _ = dir_remove_entry(parent, name)?;
    let links = inode_get_links(&inode);
    if links > 1 {
        inode_set_links(&mut inode, links - 1);
        inode_set_times(&mut inode, now());
        write_inode(child, &inode)?;
        return Ok(());
    }
    // free data blocks (direct only)
    for i in 0..12 {
        let b = inode_get_block(&inode, i);
        if b != 0 {
            free_block(b)?;
        }
    }
    // free indirect
    let ind = inode_get_block(&inode, 12);
    if ind != 0 {
        let mut bbuf = [0u8; 4096];
        read_fs_block(ind, &mut bbuf)?;
        let per = fs_block_size() as usize / 4;
        for i in 0..per {
            let off = i * 4;
            let b = read_u32_le(&bbuf, off);
            if b != 0 {
                free_block(b)?;
            }
        }
        free_block(ind)?;
    }
    // zero inode
    let empty = zero_inode();
    write_inode(child, &empty)?;
    free_inode(child)?;
    Ok(())
}

/// Remove an empty directory.
pub fn rmdir(cwd: u32, path: &str) -> Result<(), &'static str> {
    if !ext2::is_mounted() {
        return Err("not mounted");
    }
    let (parent, name) = split_parent_name(cwd, path)?;
    if name == "." || name == ".." {
        return Err("bad name");
    }
    let child = ext2::lookup(parent, name)?;
    let inode = read_inode(child)?;
    if inode_get_mode(&inode) & S_IFMT != S_IFDIR {
        return Err("not a directory");
    }
    // ensure only . and ..
    ext2::list_dir(child)?;
    let mut extras = 0;
    for i in 0..crate::fs::vfs::cache_len() {
        if let Some(n) = crate::fs::vfs::cache_get(i) {
            if n.name_str() != "." && n.name_str() != ".." {
                extras += 1;
            }
        }
    }
    if extras > 0 {
        return Err("directory not empty");
    }

    let _ = dir_remove_entry(parent, name)?;
    // free dir block
    for i in 0..12 {
        let b = inode_get_block(&inode, i);
        if b != 0 {
            free_block(b)?;
        }
    }
    let empty = zero_inode();
    write_inode(child, &empty)?;
    free_inode(child)?;

    // parent links--
    let mut pino = read_inode(parent)?;
    let links = inode_get_links(&pino);
    if links > 1 {
        inode_set_links(&mut pino, links - 1);
    }
    inode_set_times(&mut pino, now());
    write_inode(parent, &pino)?;
    Ok(())
}

