//! Phase 7b — minimal procfs mounted at `/proc`.
//!
//! Synthetic read-only files generated on open/read (no disk storage).

use crate::fs::vcore::{FileData, VfsError, FOPS_PROC};
use crate::interrupts;
use crate::memory::{free_frames, total_frames, used_frames, FRAME_SIZE};

/// Which synthetic file is open (`FileData.private`).
pub const PROC_MEMINFO: u64 = 1;
pub const PROC_MOUNTS: u64 = 2;
pub const PROC_VERSION: u64 = 3;
pub const PROC_UPTIME: u64 = 4;
pub const PROC_SELF_STATUS: u64 = 5;
pub const PROC_MODULES: u64 = 6;

const GEN_CAP: usize = 512;
static mut GEN: [u8; GEN_CAP] = [0; GEN_CAP];
static mut GEN_LEN: usize = 0;

fn gen_clear() {
    unsafe {
        GEN = [0; GEN_CAP];
        GEN_LEN = 0;
    }
}

fn gen_push(s: &str) {
    unsafe {
        for &b in s.as_bytes() {
            if GEN_LEN < GEN_CAP {
                GEN[GEN_LEN] = b;
                GEN_LEN += 1;
            }
        }
    }
}

fn gen_push_u64(n: u64) {
    let mut buf = [0u8; 20];
    let mut x = n;
    let mut i = 20;
    if x == 0 {
        gen_push("0");
        return;
    }
    while x > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    unsafe {
        for &b in &buf[i..] {
            if GEN_LEN < GEN_CAP {
                GEN[GEN_LEN] = b;
                GEN_LEN += 1;
            }
        }
    }
}

fn build_meminfo() {
    gen_clear();
    let total_kb = (total_frames() as u64).saturating_mul(FRAME_SIZE as u64) / 1024;
    let free_kb = (free_frames() as u64).saturating_mul(FRAME_SIZE as u64) / 1024;
    let used_kb = (used_frames() as u64).saturating_mul(FRAME_SIZE as u64) / 1024;
    gen_push("MemTotal:       ");
    gen_push_u64(total_kb);
    gen_push(" kB\n");
    gen_push("MemFree:        ");
    gen_push_u64(free_kb);
    gen_push(" kB\n");
    gen_push("MemAvailable:   ");
    gen_push_u64(free_kb);
    gen_push(" kB\n");
    gen_push("MemUsed:        ");
    gen_push_u64(used_kb);
    gen_push(" kB\n");
}

fn build_mounts() {
    gen_clear();
    // Mirror registered mounts (keep labels stable).
    gen_push("/dev/hda / ext2 rw 0 0\n");
    gen_push("ramfs /ram ramfs rw 0 0\n");
    gen_push("proc /proc proc rw 0 0\n");
    gen_push("devtmpfs /dev devtmpfs rw 0 0\n");
}

fn build_version() {
    gen_clear();
    gen_push("munux version 0.3 x86_64\n");
}

fn build_uptime() {
    gen_clear();
    // PIT 100 Hz → seconds as integer.tenths style: "12.34 12.34"
    let ticks = interrupts::ticks();
    let sec = ticks / 100;
    let frac = ticks % 100;
    gen_push_u64(sec);
    gen_push(".");
    if frac < 10 {
        gen_push("0");
    }
    gen_push_u64(frac);
    gen_push(" ");
    gen_push_u64(sec);
    gen_push(".");
    if frac < 10 {
        gen_push("0");
    }
    gen_push_u64(frac);
    gen_push("\n");
}

fn build_self_status() {
    gen_clear();
    let tid = crate::process::gettid();
    let tgid = crate::process::getpid();
    gen_push("Name:\t");
    // best-effort name from PCB
    let name = crate::process::with_current(|p| {
        let mut buf = [0u8; 16];
        let s = p.name_str();
        let n = s.len().min(15);
        buf[..n].copy_from_slice(s.as_bytes());
        (buf, n)
    })
    .unwrap_or(([0u8; 16], 0));
    if name.1 > 0 {
        if let Ok(s) = core::str::from_utf8(&name.0[..name.1]) {
            gen_push(s);
        }
    } else {
        gen_push("?");
    }
    gen_push("\n");
    gen_push("Pid:\t");
    gen_push_u64(tid as u64);
    gen_push("\n");
    gen_push("Tgid:\t");
    gen_push_u64(tgid as u64);
    gen_push("\n");
}

fn build_modules() {
    gen_clear();
    // Reuse GEN buffer via format_proc_modules into a temp, then push.
    // format writes Linux-ish lines; empty when no modules loaded.
    let mut tmp = [0u8; GEN_CAP];
    let n = crate::module::format_proc_modules(&mut tmp);
    unsafe {
        let take = n.min(GEN_CAP);
        GEN[..take].copy_from_slice(&tmp[..take]);
        GEN_LEN = take;
    }
}

fn rebuild(which: u64) {
    match which {
        PROC_MEMINFO => build_meminfo(),
        PROC_MOUNTS => build_mounts(),
        PROC_VERSION => build_version(),
        PROC_UPTIME => build_uptime(),
        PROC_SELF_STATUS => build_self_status(),
        PROC_MODULES => build_modules(),
        _ => gen_clear(),
    }
}

/// Open a bare proc file name (e.g. `"meminfo"`).
pub fn open_name(name: &str, readable: bool, writable: bool) -> Result<FileData, VfsError> {
    if writable {
        return Err(VfsError::Inval);
    }
    if !readable {
        return Err(VfsError::Inval);
    }
    let which = match name {
        "meminfo" => PROC_MEMINFO,
        "mounts" => PROC_MOUNTS,
        "version" => PROC_VERSION,
        "uptime" => PROC_UPTIME,
        "status" => PROC_SELF_STATUS, // when cwd is /proc/self
        "modules" => PROC_MODULES,
        _ => return Err(VfsError::NoEnt),
    };
    Ok(FileData {
        pos: 0,
        readable: true,
        writable: false,
        private: which,
        is_dir: false,
        fops_id: FOPS_PROC,
    })
}

/// Open `/proc/...` path. `path` is full path or relative under /proc.
pub fn open(path: &str, readable: bool, writable: bool) -> Result<FileData, VfsError> {
    if writable {
        // All proc files are read-only for now.
        return Err(VfsError::Inval);
    }
    if !readable {
        return Err(VfsError::Inval);
    }

    let rest = if path == "/proc" || path == "/proc/" {
        // Directory open handled in vcore.
        return Err(VfsError::IsDir);
    } else if let Some(r) = path.strip_prefix("/proc/") {
        r
    } else if let Some(r) = path.strip_prefix("proc/") {
        r
    } else {
        return Err(VfsError::NoEnt);
    };

    if rest == "self" || rest == "self/" {
        return Err(VfsError::IsDir);
    }

    let which = match rest {
        "meminfo" => PROC_MEMINFO,
        "mounts" => PROC_MOUNTS,
        "version" => PROC_VERSION,
        "uptime" => PROC_UPTIME,
        "self/status" => PROC_SELF_STATUS,
        "modules" => PROC_MODULES,
        _ => return Err(VfsError::NoEnt),
    };

    Ok(FileData {
        pos: 0,
        readable: true,
        writable: false,
        private: which,
        is_dir: false,
        fops_id: FOPS_PROC,
    })
}

pub fn read_op(f: &mut FileData, buf: &mut [u8]) -> Result<usize, VfsError> {
    // Regenerate content each read sequence from pos 0 snapshot.
    // Simple: rebuild every time (small files).
    rebuild(f.private);
    let len = unsafe { GEN_LEN };
    if f.pos as usize >= len {
        return Ok(0);
    }
    let start = f.pos as usize;
    let n = (len - start).min(buf.len());
    unsafe {
        buf[..n].copy_from_slice(&GEN[start..start + n]);
    }
    Ok(n)
}
