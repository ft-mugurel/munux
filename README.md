# munux

**munux** is a freestanding **x86_64** operating-system kernel written in **Rust** and **NASM**.  
It boots via **Multiboot / GRUB**, runs under **QEMU**, and targets a **Linux-compatible** kernel ABI (syscalls, processes, memory, VFS, and later threads and modules).

Started as a **42 KFS** learning kernel; development continues independently as **munux**.

| Branch | Role |
|--------|------|
| **`main`** | Active **x86_64** development (this tree) |
| **`32bit`** | Frozen **i686** snapshot (historical Multiboot kernel) |

Repository: [github.com/ft-mugurel/munux](https://github.com/ft-mugurel/munux)

---

## Project goal

Build a **Linux x86_64 ABI–compatible kernel in Rust** — not “run every BusyBox applet.”

BusyBox / static musl binaries are **compatibility probes** and regression tests.  
Primary architecture targets:

1. **Per-process address spaces** and a real process model  
2. **Threads** (`clone`, TID, futex, TLS)  
3. **Loadable kernel modules** (ELF loader, symbol export, init/exit)  
4. Growing syscall / VFS surface on top of that foundation  

See **[docs/ROADMAP.md](docs/ROADMAP.md)** for the phased plan.

---

## Current status (x86_64 `main`)

### Boot & build
- Multiboot → long mode trampoline → Rust `kmain`
- Custom freestanding Rust target (`#![no_std]`, panic = abort)
- Kernel linked for **x86_64**; `make iso` / `make run` / `make run-iso`
- IDE disk image (`build/disk.img`) with **ext2** rootfs (BusyBox, tools under `/bin`)

### CPU / interrupts
- **GDT** + **TSS** (ring 3 → ring 0 stack)
- **IDT** — exceptions, **IRQ0** (PIT 100 Hz), **IRQ1** (keyboard)
- Userspace entry via **`syscall` / `sysret`** (STAR / LSTAR / SCE), not `int 0x80`
- Nested `enter_user_mode` frames for cooperative fork / exec / wait

### Memory
- Multiboot memory map → **PMM** (frame bitmap)
- **4-level paging**, identity map + user mappings
- Kernel heap (`kmalloc`)
- **Still single shared address space** for processes (no per-process CR3 yet)  
  → fork+exec uses parent **user-image / mmap / stack snapshots** as a temporary bridge

### Processes & userspace
- PCB table: pid/ppid, cwd, nice, TLS (`fs_base` / `gs_base`), heap/mmap bookkeeping
- **Cooperative** `fork` / `vfork` / `execve` / `exit` / `wait4` (child often runs nested to completion)
- Private **child stacks**; parent classic stack preserved across exec when possible
- Boot handoff to userspace **`/bin/sh`** (freestanding shell); `exit` returns to kernel debug shell
- **BusyBox** static binary on disk: many core applets work; interactive **ash** works for common cases
- ~**80** Linux x86_64 syscall numbers handled (full / partial / stub) — see docs

### Filesystem & FDs
- ATA PIO **IDE** + **ext2** read/write (mkdir, touch, unlink, rmdir, link, **rename**, chmod, …)
- Virtual **`/proc`** (e.g. meminfo, mounts, pid entries) for tools like `free` / `ps` / `df`
- **Per-process FD tables** (clone on fork): files, dirs, pipes, dup/dup2

### Console
- VGA 80×25 text, PS/2 keyboard (US QWERTY)
- Userspace prompt **`$`**; kernel debug shell after sh exits

---

## Documentation

| Doc | Contents |
|-----|----------|
| **[docs/ROADMAP.md](docs/ROADMAP.md)** | Architecture roadmap (mm → schedule → threads → modules) |
| **[docs/ABI.md](docs/ABI.md)** | Syscall calling convention, process model, FD rules |
| **[docs/SYSCALL_COMPARE.md](docs/SYSCALL_COMPARE.md)** | Linux x86_64 (~385) vs munux (~80) comparison |
| **[docs/BUSYBOX_SUITE_REPORT.md](docs/BUSYBOX_SUITE_REPORT.md)** | Strict BusyBox regression suite results |
| **[docs/BUSYBOX_REPORT.md](docs/BUSYBOX_REPORT.md)** | Superseded zero-arg applet scan (historical) |
| **[SMOKE.md](SMOKE.md)** | Manual smoke checklist |

---

## Quick start

```sh
make              # build ISO + disk + boot (run-iso)
make run          # -kernel + IDE disk (faster iteration)
make iso          # build kernel.iso only
make help         # all targets
make size         # kernel / ISO size report
```

### Useful targets

| Target | Description |
|--------|-------------|
| `build` | Release kernel → `build/kernel.bin` |
| `build_debug` | Debug symbols |
| `run` | QEMU `-kernel` + `disk.img` on IDE index 0 |
| `run-iso` | GRUB ISO (cdrom) + disk |
| `iso` | Produce `build/kernel.iso` |
| `disk` | Recreate ext2 `build/disk.img` (+ rootfs tools) |
| `debug` / `debug-gdb` | QEMU GDB stub + `gdb/kfs.gdb` |
| `size` | Print artifact sizes |
| `clean` / `fclean` / `re` | Cleanup / rebuild |

**IDE layout:** primary master (`index=0`) = ext2 disk; ISO uses `index=1` as cdrom.

**Headless automation:** [qemu-connect](https://github.com/) (external) + `scripts/busybox_suite.py` for the strict suite.

---

## Userspace cheat sheet

### Freestanding `/bin/sh` (default after boot)

| Input | Behavior |
|-------|----------|
| `help` | Builtins |
| `cd` / `pwd` / `clear` / `exit` | Builtins |
| `ls`, `cat`, … | `fork` + `execve` of `/bin/<cmd>` (embedded or disk) |
| `busybox …` | Static BusyBox from rootfs |

### Kernel debug shell (after `exit` from sh)

| Command | Description |
|---------|-------------|
| `help` / `about` | Help / summary |
| `ps` / `pmm` / … | Debug dumps |
| `run sh` / `run init` | Re-enter userspace shell |
| `ls` / `cat` / … | Kernel-side FS helpers |

---

## Boot flow (x86_64)

```text
QEMU → GRUB (or -kernel) → Multiboot / long-mode entry
  → GDT + TSS
  → IDT + PIC + keyboard + PIT
  → PMM → paging → heap
  → process table + FDs + ext2 mount
  → init_syscalls (STAR/LSTAR, syscall_entry)
  → load /bin/sh → enter_user_mode
  → interactive $ shell (or kernel shell after exit)
```

### Syscall ABI (summary)

Same as **Linux x86_64**:

| Item | Value |
|------|--------|
| Entry | `syscall` |
| Number | `rax` |
| Args | `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` |
| Return | `rax` or `-errno` |
| Exit | `sysret` |

Full table and semantics: **[docs/ABI.md](docs/ABI.md)**.  
Coverage vs Linux: **[docs/SYSCALL_COMPARE.md](docs/SYSCALL_COMPARE.md)**.

---

## Project layout

```text
.
├── Makefile
├── multiboot/          # Multiboot header, exceptions, timer, syscall.asm
├── linker/linker.ld
├── grub/grub.cfg
├── userland/           # freestanding asm apps (sh, ls, cat, …)
├── scripts/            # busybox_suite.py (strict regression)
├── docs/               # ABI, roadmap, syscall compare, suite reports
├── SMOKE.md
└── src/
    ├── kernel.rs
    ├── gdt/            # GDT + TSS
    ├── interrupts/     # IDT, PIC, exceptions, keyboard, timer
    ├── memory/         # PMM, paging, heap, Multiboot
    ├── process/        # PCB, fork, memory (brk/mmap), sys
    ├── drivers/ide.rs
    ├── fs/             # ext2, path, procfs, vfs helpers
    ├── fd/             # per-process FD tables, pipes
    ├── elf/            # ELF64 load + stack/auxv
    ├── syscalls/       # Linux x86_64 dispatch
    ├── shell/          # kernel debug shell
    └── vga/
```

---

## Requirements

| Tool | Purpose |
|------|---------|
| Rust nightly (`rust-toolchain.toml`) | `build-std` / freestanding |
| nasm, ld | ASM + final link |
| grub-mkrescue + GRUB modules | ISO |
| **qemu-system-x86_64** | Emulation |
| e2fsprogs (`mkfs.ext2`) | Disk image |

```sh
rustup toolchain install nightly
# also: nasm, binutils, grub-pc-bin, xorriso, qemu-system-x86, e2fsprogs
```

---

## Smoke / regression

1. Manual checklist: **[SMOKE.md](SMOKE.md)**  
2. Strict BusyBox suite: `scripts/busybox_suite.py` → report in `docs/BUSYBOX_SUITE_REPORT.md`  
3. Headless: build `make iso`, then qemu-connect (or equivalent) with prompt `$`

---

## Known limitations (current)

- **No per-process page tables** (shared AS; snapshot hacks around fork/exec)
- **No preemptive scheduler** (cooperative nested user sessions)
- **No real threads** (`clone` / futex not implemented; `gettid` ≈ `getpid`)
- **No loadable kernel modules**
- Signals mostly **stubs** (handlers accepted; no full delivery / `rt_sigreturn`)
- **No networking**
- Subset of Linux syscalls (~80 of ~385); rest **`-ENOSYS`**
- VGA only (no serial console yet); US QWERTY only

These are intentional “next foundation” items — see the roadmap.

---

## License / acknowledgements

Licensing not fully specified in-tree. Descended from the **42** KFS track (bare-metal boot, interrupts, memory, minimal OS services). Continuing as **munux**.
