//! Phase 8 — loadable kernel modules.
//!
//! Conceptual Linux LKM lifecycle without mainline `.ko` binary compatibility:
//! export table → load container → relocate → `init()` → live → `exit()` → free.
//!
//! Admin: kernel shell + Linux `init_module` / `finit_module` / `delete_module`.
//! Formats: **MNX1** (`module::mnx`) and ELF64 **ET_REL** `.ko` (`module::elfrel`).
//! Builtin `hello` works without disk.

pub mod elfrel;
pub mod export;
pub mod mnx;

use crate::console;
use crate::fs;
use crate::memory::{
    current_cr3, kfree, kmalloc, kernel_cr3, map_page_in, switch_mm, virt_to_phys_in,
    PhysAddr, FRAME_SIZE, PAGE_KERNEL_RW,
};
use mnx::{LoadedMnx, MnxError, MNX_MAX_FILE, MNX_NAME_LEN};

/// Run module `init`/`exit` under the kernel page tables so heap-resident
/// module code is always mapped (see heap dual-map into `kernel_cr3`).
fn with_kernel_mm<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let prev = current_cr3();
    let k = kernel_cr3();
    if k != 0 && prev != k {
        switch_mm(k);
    }
    let r = f();
    if k != 0 && prev != k && prev != 0 {
        switch_mm(prev);
    }
    r
}

/// Max simultaneous loaded modules.
pub const MAX_MODULES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModuleState {
    Free = 0,
    Live = 1,
    Going = 2,
}

/// One loaded module (Linux-shaped fields, simplified).
#[derive(Clone, Copy)]
pub struct Module {
    pub used: bool,
    pub state: ModuleState,
    pub name: [u8; MNX_NAME_LEN],
    pub name_len: u8,
    pub refcount: u32,
    pub code: *mut u8,
    pub code_len: usize,
    pub init: Option<extern "C" fn() -> i32>,
    pub exit: Option<extern "C" fn() -> i32>,
    /// True if this is the in-kernel builtin (no heap code image).
    pub builtin: bool,
}

impl Module {
    const fn empty() -> Self {
        Self {
            used: false,
            state: ModuleState::Free,
            name: [0; MNX_NAME_LEN],
            name_len: 0,
            refcount: 0,
            code: core::ptr::null_mut(),
            code_len: 0,
            init: None,
            exit: None,
            builtin: false,
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("?")
    }
}

static mut MODULES: [Module; MAX_MODULES] = [Module::empty(); MAX_MODULES];
/// Slot whose `init()` is running (THIS_MODULE for `register_chrdev`).
static mut LOADING_SLOT: i32 = -1;

fn modules_mut() -> &'static mut [Module; MAX_MODULES] {
    unsafe { &mut *core::ptr::addr_of_mut!(MODULES) }
}

fn modules() -> &'static [Module; MAX_MODULES] {
    unsafe { &*core::ptr::addr_of!(MODULES) }
}

fn set_name(m: &mut Module, name: &str) {
    m.name = [0; MNX_NAME_LEN];
    let b = name.as_bytes();
    let n = b.len().min(MNX_NAME_LEN);
    m.name[..n].copy_from_slice(&b[..n]);
    m.name_len = n as u8;
}

fn find_by_name(name: &str) -> Option<usize> {
    for (i, m) in modules().iter().enumerate() {
        if m.used && m.name_str() == name {
            return Some(i);
        }
    }
    None
}

fn alloc_slot() -> Option<usize> {
    for (i, m) in modules_mut().iter_mut().enumerate() {
        if !m.used {
            return Some(i);
        }
    }
    None
}

/// Module currently running `init()` (`THIS_MODULE`), or -1.
pub fn loading_slot() -> i32 {
    unsafe { LOADING_SLOT }
}

fn set_loading(slot: i32) {
    unsafe {
        LOADING_SLOT = slot;
    }
}

/// Increment live refcount (open `/dev` node owned by this module).
pub fn try_get(slot: usize) -> bool {
    let m = match modules_mut().get_mut(slot) {
        Some(m) if m.used => m,
        _ => return false,
    };
    m.refcount = m.refcount.saturating_add(1);
    true
}

/// Decrement live refcount (close last dup of a module-owned file).
pub fn put(slot: usize) {
    if let Some(m) = modules_mut().get_mut(slot) {
        if m.used && m.refcount > 0 {
            m.refcount -= 1;
        }
    }
}

/// Map every live module code image into the current CR3 (from kernel tables).
///
/// Needed so fops in a later process can execute heap-resident module code
/// without switching away from the user buffer's address space.
pub fn map_code_into_current() {
    let cur = current_cr3();
    let k = kernel_cr3();
    if k == 0 || cur == 0 || cur == k {
        return;
    }
    for m in modules().iter() {
        if !m.used || m.builtin || m.code.is_null() || m.code_len == 0 {
            continue;
        }
        let start = (m.code as u64) & !(FRAME_SIZE as u64 - 1);
        let end = (m.code as u64 + m.code_len as u64 + FRAME_SIZE as u64 - 1)
            & !(FRAME_SIZE as u64 - 1);
        let mut v = start;
        while v < end {
            if virt_to_phys_in(cur, v).is_none() {
                if let Some(phys) = virt_to_phys_in(k, v) {
                    map_page_in(
                        cur,
                        v,
                        PhysAddr::new(phys & !0xFFF),
                        PAGE_KERNEL_RW,
                    );
                }
            }
            v = v.wrapping_add(FRAME_SIZE as u64);
            if v == 0 {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Builtin hello (no file needed — proves lifecycle even without disk modules)
// ---------------------------------------------------------------------------

extern "C" fn builtin_hello_init() -> i32 {
    console::println("hello: module loaded (builtin)");
    0
}

extern "C" fn builtin_hello_exit() -> i32 {
    console::println("hello: module unloaded (builtin)");
    0
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleError {
    Exists,
    NotFound,
    Busy,
    NoSlot,
    BadPath,
    Io,
    Format(MnxError),
    InitFail,
}

impl ModuleError {
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleError::Exists => "already loaded",
            ModuleError::NotFound => "not found",
            ModuleError::Busy => "in use",
            ModuleError::NoSlot => "module table full",
            ModuleError::BadPath => "bad path",
            ModuleError::Io => "I/O error",
            ModuleError::Format(e) => e.as_str(),
            ModuleError::InitFail => "init failed",
        }
    }
}

fn strip_mod_suffix(base: &str) -> &str {
    base.strip_suffix(".ko")
        .or_else(|| base.strip_suffix(".mnx"))
        .unwrap_or(base)
}

/// Load a module by name or path.
///
/// - `hello` → `/lib/modules/hello.ko`, then `.mnx`, else builtin `hello`
/// - `/path/to/foo.ko` or `.mnx` → load that file (ELF ET_REL or MNX1)
/// - bare name `foo` → try `.ko` then `.mnx` under `/lib/modules/`
pub fn insmod(arg: &str) -> Result<(), ModuleError> {
    let arg = arg.trim();
    if arg.is_empty() {
        return Err(ModuleError::BadPath);
    }

    // Derive module name for duplicate check / table.
    let name = if arg.starts_with('/') {
        let base = arg.rsplit('/').next().unwrap_or(arg);
        strip_mod_suffix(base)
    } else {
        strip_mod_suffix(arg)
    };
    if name.is_empty() || name.len() > MNX_NAME_LEN {
        return Err(ModuleError::BadPath);
    }
    if find_by_name(name).is_some() {
        return Err(ModuleError::Exists);
    }

    if arg.starts_with('/') {
        return try_load_file(arg, name);
    }

    // Bare name: .ko (ELF) then .mnx then builtin hello.
    let ko = format_module_path(name, b".ko");
    match try_load_file(ko.as_str(), name) {
        Ok(()) => return Ok(()),
        Err(ModuleError::NotFound) | Err(ModuleError::Io) => {}
        Err(e) => return Err(e),
    }
    let mnx = format_module_path(name, b".mnx");
    match try_load_file(mnx.as_str(), name) {
        Ok(()) => Ok(()),
        Err(ModuleError::NotFound) | Err(ModuleError::Io) => {
            if name == "hello" {
                load_builtin("hello", builtin_hello_init, Some(builtin_hello_exit))
            } else {
                Err(ModuleError::NotFound)
            }
        }
        Err(e) => Err(e),
    }
}

/// Small path buffer: `/lib/modules/<name>.<ext>`
struct PathBuf {
    data: [u8; 64],
    len: usize,
}

impl PathBuf {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }
}

fn format_module_path(name: &str, suffix: &[u8]) -> PathBuf {
    let mut p = PathBuf {
        data: [0; 64],
        len: 0,
    };
    let prefix = b"/lib/modules/";
    let nb = name.as_bytes();
    if prefix.len() + nb.len() + suffix.len() > p.data.len() {
        return p;
    }
    p.data[..prefix.len()].copy_from_slice(prefix);
    p.data[prefix.len()..prefix.len() + nb.len()].copy_from_slice(nb);
    p.data[prefix.len() + nb.len()..prefix.len() + nb.len() + suffix.len()]
        .copy_from_slice(suffix);
    p.len = prefix.len() + nb.len() + suffix.len();
    p
}

fn try_load_file(path: &str, name: &str) -> Result<(), ModuleError> {
    if path.is_empty() {
        return Err(ModuleError::BadPath);
    }
    if !fs::is_ready() || !fs::vcore::is_ready() {
        return Err(ModuleError::Io);
    }
    let mut f = match fs::vcore::vfs_open(path, 0, true, false) {
        Ok(f) if !f.is_dir => f,
        Ok(_) => return Err(ModuleError::BadPath),
        Err(_) => return Err(ModuleError::NotFound),
    };

    // Read whole file into a temporary heap buffer.
    let tmp = kmalloc(MNX_MAX_FILE).ok_or(ModuleError::Format(MnxError::Oom))?;
    let mut total = 0usize;
    loop {
        if total >= MNX_MAX_FILE {
            break;
        }
        let slice = unsafe {
            core::slice::from_raw_parts_mut(tmp.add(total), MNX_MAX_FILE - total)
        };
        match fs::vcore::vfs_read(&mut f, slice) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(_) => {
                kfree(tmp);
                return Err(ModuleError::Io);
            }
        }
    }
    let bytes = unsafe { core::slice::from_raw_parts(tmp, total) };
    let loaded = match load_image(bytes, name) {
        Ok(l) => l,
        Err(e) => {
            kfree(tmp);
            return Err(ModuleError::Format(e));
        }
    };
    kfree(tmp);

    // Prefer name from header if non-empty; else path-derived.
    let mut name_buf = [0u8; MNX_NAME_LEN];
    let nlen = loaded.name_len.min(MNX_NAME_LEN);
    name_buf[..nlen].copy_from_slice(&loaded.name[..nlen]);
    let header_name = core::str::from_utf8(&name_buf[..nlen]).unwrap_or("");
    let final_name = if !header_name.is_empty() {
        header_name
    } else {
        name
    };
    if find_by_name(final_name).is_some() {
        unsafe {
            mnx::free_code(loaded.code);
        }
        return Err(ModuleError::Exists);
    }

    install_loaded(final_name, loaded, false)
}

fn load_builtin(
    name: &str,
    init: extern "C" fn() -> i32,
    exit: Option<extern "C" fn() -> i32>,
) -> Result<(), ModuleError> {
    if find_by_name(name).is_some() {
        return Err(ModuleError::Exists);
    }
    let slot = alloc_slot().ok_or(ModuleError::NoSlot)?;
    {
        let m = &mut modules_mut()[slot];
        m.used = true;
        m.state = ModuleState::Live;
        set_name(m, name);
        m.refcount = 0;
        m.code = core::ptr::null_mut();
        m.code_len = 0;
        m.init = Some(init);
        m.exit = exit;
        m.builtin = true;
    }
    set_loading(slot as i32);
    let rc = with_kernel_mm(|| init());
    set_loading(-1);
    if rc != 0 {
        modules_mut()[slot] = Module::empty();
        return Err(ModuleError::InitFail);
    }
    console::print("module: loaded ");
    console::print(name);
    console::println(" (builtin)");
    Ok(())
}

fn install_loaded(name: &str, loaded: LoadedMnx, builtin: bool) -> Result<(), ModuleError> {
    let slot = match alloc_slot() {
        Some(s) => s,
        None => {
            unsafe {
                mnx::free_code(loaded.code);
            }
            return Err(ModuleError::NoSlot);
        }
    };

    {
        let m = &mut modules_mut()[slot];
        m.used = true;
        m.state = ModuleState::Live;
        set_name(m, name);
        m.refcount = 0;
        m.code = loaded.code;
        m.code_len = loaded.code_len;
        m.init = loaded.init;
        m.exit = loaded.exit;
        m.builtin = builtin;
    }

    set_loading(slot as i32);
    let rc = if let Some(init) = loaded.init {
        with_kernel_mm(|| init())
    } else {
        0
    };
    set_loading(-1);
    if rc != 0 {
        unsafe {
            mnx::free_code(loaded.code);
        }
        modules_mut()[slot] = Module::empty();
        return Err(ModuleError::InitFail);
    }

    console::print("module: loaded ");
    console::print(name);
    console::print(" code=");
    console::write_u64(loaded.code_len as u64);
    console::println("B");
    Ok(())
}

/// Detect MNX1 vs ELF64 ET_REL and relocate into a heap image.
fn load_image(bytes: &[u8], name_hint: &str) -> Result<mnx::LoadedMnx, MnxError> {
    if elfrel::is_elf(bytes) {
        elfrel::load_from_bytes(bytes, name_hint)
    } else {
        mnx::load_from_bytes(bytes)
    }
}

/// Load a module image already in a kernel-accessible buffer (for `init_module`).
///
/// `name_hint` is used if the container has no embedded name (MNX header /
/// ELF `.modinfo name=`).
pub fn init_from_image(bytes: &[u8], name_hint: &str) -> Result<(), ModuleError> {
    if bytes.is_empty() {
        return Err(ModuleError::Format(MnxError::Truncated));
    }
    let loaded = load_image(bytes, name_hint).map_err(ModuleError::Format)?;

    let mut name_buf = [0u8; MNX_NAME_LEN];
    let nlen = loaded.name_len.min(MNX_NAME_LEN);
    name_buf[..nlen].copy_from_slice(&loaded.name[..nlen]);
    let header_name = core::str::from_utf8(&name_buf[..nlen]).unwrap_or("");
    let final_name = if !header_name.is_empty() {
        header_name
    } else if !name_hint.is_empty() {
        name_hint
    } else {
        "module"
    };
    if find_by_name(final_name).is_some() {
        unsafe {
            mnx::free_code(loaded.code);
        }
        return Err(ModuleError::Exists);
    }
    install_loaded(final_name, loaded, false)
}

/// Unload module by name. Fails if refcount > 0.
pub fn rmmod(name: &str) -> Result<(), ModuleError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ModuleError::BadPath);
    }
    let idx = find_by_name(name).ok_or(ModuleError::NotFound)?;
    let m = &mut modules_mut()[idx];
    if m.refcount > 0 {
        return Err(ModuleError::Busy);
    }
    m.state = ModuleState::Going;
    if let Some(exit) = m.exit {
        let _ = with_kernel_mm(|| exit());
    }
    if !m.builtin && !m.code.is_null() {
        unsafe {
            mnx::free_code(m.code);
        }
    }
    *m = Module::empty();
    console::print("module: unloaded ");
    console::println(name);
    Ok(())
}

/// Print loaded modules (lsmod).
pub fn lsmod() {
    console::println("Module                  Size  Used  State");
    let mut any = false;
    for m in modules().iter() {
        if !m.used {
            continue;
        }
        any = true;
        // name padded roughly
        console::print("  ");
        console::print(m.name_str());
        let pad = 22usize.saturating_sub(m.name_len as usize);
        for _ in 0..pad {
            console::print(" ");
        }
        console::write_u64(m.code_len as u64);
        console::print("  ");
        console::write_u64(m.refcount as u64);
        console::print("  ");
        match m.state {
            ModuleState::Live => console::print("Live"),
            ModuleState::Going => console::print("Going"),
            ModuleState::Free => console::print("Free"),
        }
        if m.builtin {
            console::print(" [builtin]");
        }
        console::println("");
    }
    if !any {
        console::println("  (none)");
    }
    console::print("exports=");
    console::write_u64(export::count() as u64);
    console::println("");
}

/// Number of live modules.
pub fn live_count() -> usize {
    modules().iter().filter(|m| m.used).count()
}

/// Snapshot one live module for `/proc/modules` / userspace lsmod.
pub fn live_at(i: usize) -> Option<( [u8; MNX_NAME_LEN], u8, usize, u32 )> {
    let mut n = 0usize;
    for m in modules().iter() {
        if !m.used {
            continue;
        }
        if n == i {
            return Some((m.name, m.name_len, m.code_len, m.refcount));
        }
        n += 1;
    }
    None
}

/// Format Linux-ish `/proc/modules` lines into `out`. Returns bytes written.
///
/// Format: `name size refcount deps state address` (deps always `-`).
pub fn format_proc_modules(out: &mut [u8]) -> usize {
    let mut w = 0usize;
    let mut i = 0usize;
    while let Some((name, nlen, size, refs)) = live_at(i) {
        i += 1;
        let nm = core::str::from_utf8(&name[..nlen as usize]).unwrap_or("?");
        // name
        for &b in nm.as_bytes() {
            if w < out.len() {
                out[w] = b;
                w += 1;
            }
        }
        push_byte(out, &mut w, b' ');
        push_u64(out, &mut w, size as u64);
        push_byte(out, &mut w, b' ');
        push_u64(out, &mut w, refs as u64);
        // " - Live 0x0\n"
        for &b in b" - Live 0x0\n" {
            push_byte(out, &mut w, b);
        }
    }
    w
}

fn push_byte(out: &mut [u8], w: &mut usize, b: u8) {
    if *w < out.len() {
        out[*w] = b;
        *w += 1;
    }
}

fn push_u64(out: &mut [u8], w: &mut usize, mut n: u64) {
    let mut buf = [0u8; 20];
    let mut i = 20;
    if n == 0 {
        push_byte(out, w, b'0');
        return;
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    for &b in &buf[i..] {
        push_byte(out, w, b);
    }
}

/// Optional boot banner.
pub fn init() {
    console::print("module: exports=");
    console::write_u64(export::count() as u64);
    console::print(" slots=");
    console::write_u64(MAX_MODULES as u64);
    console::println("");
}
