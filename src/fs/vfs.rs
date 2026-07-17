//! Generic filesystem node structure (subject checklist).

/// File / directory type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum NodeType {
    Unknown = 0,
    Regular = 1,
    Directory = 2,
    Symlink = 3,
    CharDev = 4,
    BlockDev = 5,
    Fifo = 6,
    Socket = 7,
}

impl NodeType {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Unknown => "unknown",
            NodeType::Regular => "file",
            NodeType::Directory => "dir",
            NodeType::Symlink => "link",
            NodeType::CharDev => "char",
            NodeType::BlockDev => "block",
            NodeType::Fifo => "fifo",
            NodeType::Socket => "sock",
        }
    }
}

pub const MAX_NAME: usize = 28;
pub const MAX_CHILDREN: usize = 16;

/// Complete filesystem node (kernel-side directory tree entry).
///
/// Subject fields:
/// name, size, type, inode, links, master, father, children, rights, next of kin
#[derive(Clone, Copy)]
pub struct FsNode {
    /// File name (UTF-8 bytes, null-padded)
    pub name: [u8; MAX_NAME],
    /// Size in bytes
    pub size: u32,
    /// Node type
    pub kind: NodeType,
    /// Inode number
    pub inode: u32,
    /// Hard link count
    pub links: u16,
    /// Master / filesystem root inode (usually 2 for ext2)
    pub master: u32,
    /// Parent directory inode (father)
    pub father: u32,
    /// Child inode numbers (directories)
    pub children: [u32; MAX_CHILDREN],
    pub nchildren: u8,
    /// Permission bits (Unix mode low 12 bits)
    pub rights: u16,
    /// Next sibling inode in the same directory ("next of kin")
    pub next_kin: u32,
    pub used: bool,
}

impl FsNode {
    pub const fn empty() -> Self {
        Self {
            name: [0; MAX_NAME],
            size: 0,
            kind: NodeType::Unknown,
            inode: 0,
            links: 0,
            master: 0,
            father: 0,
            children: [0; MAX_CHILDREN],
            nchildren: 0,
            rights: 0,
            next_kin: 0,
            used: false,
        }
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; MAX_NAME];
        for (i, b) in s.bytes().take(MAX_NAME - 1).enumerate() {
            self.name[i] = b;
        }
    }

    pub fn name_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..len]).unwrap_or("?")
    }
}

/// Small cache of FsNodes filled from ext2 walks.
pub const NODE_CACHE: usize = 64;
static mut CACHE: [FsNode; NODE_CACHE] = [FsNode::empty(); NODE_CACHE];
static mut CACHE_LEN: usize = 0;

pub fn cache_clear() {
    unsafe {
        for i in 0..NODE_CACHE {
            CACHE[i] = FsNode::empty();
        }
        CACHE_LEN = 0;
    }
}

pub fn cache_push(node: FsNode) {
    unsafe {
        if CACHE_LEN < NODE_CACHE {
            CACHE[CACHE_LEN] = node;
            CACHE_LEN += 1;
        }
    }
}

pub fn cache_get(i: usize) -> Option<FsNode> {
    unsafe {
        if i < CACHE_LEN {
            Some(CACHE[i])
        } else {
            None
        }
    }
}

pub fn cache_len() -> usize {
    unsafe { CACHE_LEN }
}

pub fn cache_find_name(father: u32, name: &str) -> Option<FsNode> {
    unsafe {
        for i in 0..CACHE_LEN {
            let n = &CACHE[i];
            if n.used && n.father == father && n.name_str() == name {
                return Some(*n);
            }
        }
    }
    None
}
