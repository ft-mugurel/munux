# munux

**munux** is a freestanding **x86_64** operating-system kernel written in **Rust** and **NASM**.  
It boots via **Multiboot / GRUB**, runs under **QEMU**, and targets a **Linux-compatible** kernel ABI (syscalls, processes, threads, memory, and later VFS + modules).

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

1. ~~**Per-process address spaces** and a real process model~~ ✅  
2. ~~**Threads** (`clone`, TID, futex, TLS) + signals~~ ✅ (practical slices)  
3. ~~**Loadable kernel modules** (export table, init/exit, chardev)~~ ✅ practical (MNX1, not mainline `.ko`)  
4. Growing syscall / VFS surface on top of that foundation (Phase 9)  

See **[docs/ROADMAP.md](docs/ROADMAP.md)** for the phased plan.

---

## Current status (x86_64 `main`)

**Phases 1–8 practical** are in place: mm, preempt, threads, signals, futex,
VFS (incl. pipes/vops), and **loadable modules** (MNX1 + Linux `init_module` /
`delete_module` / `finit_module`, `/bin/insmod|rmmod|lsmod`, `hello.mnx`,
`echo.mnx` → `/dev/echo` with unload refcount).

**Next epic: Phase 9** (broader Linux surface). Optional P8 polish: ELF ET_REL
`.ko`-style loader (not required for the north-star demo).

### Boot & build
- Multiboot → long mode trampoline → Rust `kmain`
- Custom freestanding Rust target (`#![no_std]`, panic = abort)
- Kernel linked for **x86_64**; `make iso` / `make run` / `make run-iso`
- IDE disk image (`build/disk.img`) with **ext2** rootfs (BusyBox, tools under `/bin`)

### CPU / interrupts
- **GDT** + **TSS** (per-process kernel stacks / RSP0)
- **IDT** — exceptions, **IRQ0** (PIT 100 Hz), **IRQ1** (keyboard)
- Userspace entry via **`syscall` / `sysret`** (STAR / LSTAR / SCE)
- Nested `enter_user_mode` for wait/exec; IRQ preemption via `TrapFrame` (nest depth ≤ 1)

### Memory
- Multiboot memory map → **PMM** (frame bitmap)
- **4-level paging**, identity kernel window + **per-process CR3** (`clone_mm` on fork)
- Kernel heap (`kmalloc`)
- Shared mm for threads (`CLONE_VM`); free only when last user exits

### Processes, threads, signals, sync
- PCB: **tid / tgid**, cwd, TLS, traps, signal masks/handlers, clear_child_tid
- **`fork` / `clone` / `execve` / `exit` / `exit_group` / `wait4`**
- **`gettid`** ≠ **`getpid`** (tgid) for threads
- Preemptive **user→user** timer switches; cooperative wait under deep nest
- **Signals:** `kill` / `tkill` / `tgkill`, `rt_sigaction` / `rt_sigprocmask` / `rt_sigreturn`, default terminate + user handlers; **Ctrl-C → SIGINT**
- **Futex:** `FUTEX_WAIT` / `FUTEX_WAKE` (+ PRIVATE); clear_child_tid wake on exit
- Boot handoff to **`/bin/sh`**; shell ignores SIGINT/SIGQUIT; `exit` → kernel debug shell
- **BusyBox** static binary on disk for regression probes

### Filesystem & FDs
- **VFS (P7 practical):** fops, mounts (ext2/ramfs/proc), chrdev, blkdev `hda`,
  vops (mkdir/unlink/rename/link), pipes, dup/dup2
- ATA PIO **IDE** via blockdev; **ext2** via VFS; **`/proc`** + **`/dev`** virtual
- FD tables: clone/share; open/read/write via VFS

### Modules (P8 practical)
- `src/module/`: `struct module`, export table, **MNX1** loader + relocs
- Syscalls: `init_module` (175), `delete_module` (176), `finit_module` (313)
- `/proc/modules`; userspace `/bin/insmod` `/bin/rmmod` `/bin/lsmod`
- `hello.mnx` (printk); `echo.mnx` registers `/dev/echo` via C-ABI `register_chrdev`
- Unload blocked while the device is open (`echotest` checks EBUSY)
- Kernel debug shell also has `insmod` / `rmmod` / `lsmod` (after `exit` from sh)

### Console
- VGA 80×25 text, PS/2 keyboard (US QWERTY)
- Userspace prompt **`$`**; kernel debug shell after sh exits

---

## Documentation

| Doc | Contents |
|-----|----------|
| **[docs/ROADMAP.md](docs/ROADMAP.md)** | Architecture roadmap (mm → schedule → threads → modules) |
| **[docs/MM.md](docs/MM.md)** | Memory layout + phase checklist |
| **[docs/ABI.md](docs/ABI.md)** | Syscall calling convention, process model, FD rules |
| **[docs/SYSCALL_COMPARE.md](docs/SYSCALL_COMPARE.md)** | Linux x86_64 vs munux syscall coverage |
| **[docs/SMOKE_PREEMPT.md](docs/SMOKE_PREEMPT.md)** | IRQ preemption tests (`preempttest`) |
| **[docs/SMOKE_CLONE.md](docs/SMOKE_CLONE.md)** | `clone` / tid smoke |
| **[docs/SMOKE_SIGNAL.md](docs/SMOKE_SIGNAL.md)** | Signals + Ctrl-C |
| **[docs/SMOKE_FUTEX.md](docs/SMOKE_FUTEX.md)** | Futex join smoke |
| **[docs/SMOKE_VFS.md](docs/SMOKE_VFS.md)** | VFS mounts / fops / pipes (Phase 7) |
| **[docs/SMOKE_MODULE.md](docs/SMOKE_MODULE.md)** | Modules: insmod/rmmod/lsmod, hello + `/dev/echo` |
| **[docs/BUSYBOX_SUITE_REPORT.md](docs/BUSYBOX_SUITE_REPORT.md)** | Strict BusyBox regression suite |
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
| `insmod` / `rmmod` / `lsmod` / `echotest` | Loadable modules (need disk `.mnx`) |
| `busybox …` | Static BusyBox from rootfs |

### Kernel debug shell (after `exit` from sh)

| Command | Description |
|---------|-------------|
| `help` / `about` | Help / summary |
| `ps` / `pmm` / `vfs` | Debug dumps |
| `insmod` / `rmmod` / `lsmod` | Same module core as userspace (bare `hello` can be builtin) |
| `run sh` / `run init` | Re-enter userspace shell |
| `ls` / `cat` / … | Kernel-side FS helpers |

---

## Boot flow (x86_64)

```text
QEMU → GRUB (or -kernel) → Multiboot / long-mode entry
  → GDT + TSS
  → IDT + PIC + keyboard + PIT
  → PMM → paging → heap
  → process table + FDs + ext2 mount + VFS + module subsystem
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
├── userland/           # freestanding asm apps (sh, ls, cat, insmod, …)
├── modules/            # MNX1 sources (hello.asm, echo.asm)
├── scripts/            # busybox_suite.py (strict regression)
├── docs/               # ABI, roadmap, syscall compare, suite reports
├── SMOKE.md
└── src/
    ├── kernel.rs
    ├── gdt/            # GDT + TSS
    ├── interrupts/     # IDT, PIC, exceptions, keyboard, timer
    ├── memory/         # PMM, paging (clone_mm), heap, Multiboot
    ├── process/        # PCB, fork, clone, sched, signals, futex, sys
    ├── module/         # loadable modules (export, MNX1, list)
    ├── tty.rs          # Ctrl-C → SIGINT hooks
    ├── drivers/ide.rs
    ├── fs/             # ext2, path, procfs, vcore/vops, vfs
    ├── fd/             # FD tables + pipe
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
2. Focused tests (userspace / kernel shell):
   - `$ signaltest` · `$ clonetest` · `$ futextest` · `$ forktest`
   - `$ insmod /lib/modules/echo.mnx` · `$ echotest` · `$ rmmod echo`
   - `munux> preempttest` (IRQ preemption A–G)
3. Strict BusyBox suite: `scripts/busybox_suite.py` → `docs/BUSYBOX_SUITE_REPORT.md`  
4. Headless: `make iso disk`, then **qemu-connect** with prompt `$`

### Quick smoke after boot

```text
$ signaltest          # caught + parent ok
$ clonetest
$ futextest
$ forktest
$ insmod /lib/modules/hello.mnx
$ lsmod
$ rmmod hello
$ insmod /lib/modules/echo.mnx
$ echotest            # PASS + EBUSY while open
$ rmmod echo
$ busybox true
$ busybox sleep 30    # optional: Ctrl+C should return to $
$ exit
munux> preempttest    # pass=7 fail=0
```

Need **`make disk`** so `/lib/modules/*.mnx` and `/bin/insmod` are on the image.

---

## Known limitations (current)

**Modules (P8 — practical, not Linux-complete)**
- Format is **MNX1**, not a mainline Linux **`.ko`** (no ET_REL, vermagic, GPL symbols)
- No `depmod` / module dependency tree, signing, livepatch
- IDE is still a **built-in**, not a loadable driver
- Heap dual-map + CR3 switch is a teaching workaround (not Linux `vmalloc` + shared kernel PDPT)

**Process / MM**
- **No full Linux signal frame** (`siginfo` / `ucontext` / SA_NODEFER / RT signals)
- **No futex timeout / requeue / PI**; pthread mutex soak incomplete
- **No in-kernel preemption**; IRQ preempt gated under deep nest (depth ≥ 2 cooperative)
- **No higher-half kernel**; identity map + high heap for modules
- **No file-backed `mmap`** (anonymous mmap exists)

**VFS / syscalls / HW**
- No `mount`/`umount` syscalls; no full dentry cache
- **No networking**, no SMP
- Subset of Linux syscalls; rest **`-ENOSYS`**
- VGA only (no serial console yet); US QWERTY only

See **[docs/ROADMAP.md](docs/ROADMAP.md)** (Phase 9 next) and **[docs/SMOKE_MODULE.md](docs/SMOKE_MODULE.md)**.

---

## License / acknowledgements

Licensing not fully specified in-tree. Descended from the **42** KFS track (bare-metal boot, interrupts, memory, minimal OS services). Continuing as **munux**.
