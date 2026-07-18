//! Working directory and path helpers (per-process cwd via process table).

use crate::fs::ext2;
use crate::process;

/// Fallback when process table is not yet initialized.
static mut CWD_INODE: u32 = 2;

pub fn cwd_inode() -> u32 {
    // Prefer current process cwd (U5+).
    if process::process_count() > 0 {
        process::get_cwd_inode()
    } else {
        unsafe { CWD_INODE }
    }
}

pub fn set_cwd_inode(ino: u32) {
    if process::process_count() > 0 {
        process::set_cwd_inode(ino);
    }
    unsafe {
        CWD_INODE = ino;
    }
}

pub fn chdir(path: &str) -> Result<(), &'static str> {
    let cur = cwd_inode();
    let ino = ext2::resolve_path(cur, path)?;
    if !ext2::inode_is_dir(ino) {
        return Err("not a directory");
    }
    set_cwd_inode(ino);
    Ok(())
}

/// Write absolute path of cwd into `out` (NUL-terminated). Returns length.
pub fn getcwd_pretty(out: &mut [u8]) -> usize {
    let target = cwd_inode();
    if target == ext2::ROOT_INODE {
        if !out.is_empty() {
            out[0] = b'/';
            if out.len() > 1 {
                out[1] = 0;
            }
            return 1;
        }
        return 0;
    }

    let mut names: [[u8; 28]; 16] = [[0; 28]; 16];
    let mut depths = 0usize;
    let mut cur = target;

    for _ in 0..16 {
        if cur == ext2::ROOT_INODE {
            break;
        }
        let parent = match ext2::lookup(cur, "..") {
            Ok(p) => p,
            Err(_) => break,
        };
        let _ = ext2::list_dir(parent);
        let mut name_buf = [0u8; 28];
        let mut got = false;
        for i in 0..crate::fs::vfs::cache_len() {
            if let Some(n) = crate::fs::vfs::cache_get(i) {
                if n.inode == cur {
                    let s = n.name_str();
                    if s != "." && s != ".." {
                        for (j, b) in s.bytes().take(27).enumerate() {
                            name_buf[j] = b;
                        }
                        got = true;
                        break;
                    }
                }
            }
        }
        if !got {
            // fallback #ino
            let mut n = cur;
            let mut tmp = [0u8; 10];
            let mut t = 0;
            while n > 0 && t < 10 {
                tmp[t] = b'0' + (n % 10) as u8;
                n /= 10;
                t += 1;
            }
            let mut j = 0;
            name_buf[j] = b'#';
            j += 1;
            while t > 0 && j < 27 {
                t -= 1;
                name_buf[j] = tmp[t];
                j += 1;
            }
        }
        if depths < 16 {
            names[depths] = name_buf;
            depths += 1;
        }
        if parent == cur {
            break;
        }
        cur = parent;
    }

    let mut len = 0usize;
    if len < out.len() {
        out[len] = b'/';
        len += 1;
    }
    for d in (0..depths).rev() {
        if len > 1 && len < out.len() && out[len - 1] != b'/' {
            out[len] = b'/';
            len += 1;
        }
        for &b in names[d].iter() {
            if b == 0 {
                break;
            }
            if len < out.len() - 1 {
                out[len] = b;
                len += 1;
            }
        }
    }
    if len == 0 && !out.is_empty() {
        out[0] = b'/';
        len = 1;
    }
    if len < out.len() {
        out[len] = 0;
    }
    len
}
