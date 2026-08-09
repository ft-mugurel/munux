# munux

**munux** is a freestanding **x86_64** operating-system kernel written in **Rust** and **NASM**.  
It boots via **Multiboot / GRUB**, runs under **QEMU**, and targets **Linux userspace results**: same syscall ABI, same programs, eventually the same desktop — clang vs gcc, not a Linux source clone.

Started as a **42 KFS** learning kernel; development continues independently as **munux**.

| Branch | Role |
|--------|------|
| **`main`** | Active **x86_64** development (this tree) |
| **`32bit`** | Frozen **i686** snapshot (historical Multiboot kernel) |

Repository: [github.com/ft-mugurel/munux](https://github.com/ft-mugurel/munux)

---

## Project goal

Build a **Linux x86_64 kernel in Rust** that you can treat like Linux: **install a Linux desktop environment and use the system as a Linux machine**.

Internals may differ (Rust vs C, our module format vs mainline `.ko`, different VFS/scheduler). **Results must not** — same syscall numbers and structs, same process/thread/file/mmap/ELF behavior, same userspace.

BusyBox / static musl binaries are **probes and regression tests**, not the product.

**Path (see [docs/ROADMAP.md](docs/ROADMAP.md)):**

1. ~~**Per-process address spaces** and a real process model~~ ✅  
2. ~~**Threads** (`clone`, TID, futex, TLS) + signals~~ ✅ (practical slices)  
3. ~~**Loadable kernel modules** (export table, init/exit, chardev)~~ ✅ practical (MNX1 + ELF ET_REL `.ko`)  
4. **Phase 9–10** — Linux surface; **P10a–d** dynlink + glibc `hello_dyn` + `clone3`/TLS  
5. **linuxkpi** — compile Linux driver `.c` ([docs/LINUXKPI.md](docs/LINUXKPI.md)); not distro `.ko` blobs  
6. **Later** — more libc (pthread soak), PTYs/job control, sockets, graphics+input, **installable desktop**

---

## Current status (x86_64 `main`)

**Phases 1–8c** are in place: mm, preempt, threads, signals, futex,
VFS (incl. pipes/vops), and **loadable modules** (MNX1 **and** ELF64 ET_REL `.ko`
+ Linux `init_module` / `delete_module` / `finit_module`, `/bin/insmod|rmmod|lsmod`,
`hello.{mnx,ko}`, `echo.{mnx,ko}` → `/dev/echo` with unload refcount).

**Phase 9a–9e** in; **P10a–d** dynlink: `dynlinktest` / `dynlinkpie` / **glibc `hello_dyn`** / `tlsclone` / glibc `clonec`. Next: pthread soak, then P11 PTYs.  
Destination remains a **Linux desktop** (P10–P14). IDE stays built-in.

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
- **`fork` / `clone` / `clone3` / `execve` / `exit` / `exit_group` / `wait4`**
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

### Modules (P8a–8c)
- `src/module/`: `struct module`, export table, **MNX1** + **ELF64 ET_REL** `.ko`
- Syscalls: `init_module` (175), `delete_module` (176), `finit_module` (313)
- `/proc/modules`; userspace `/bin/insmod` `/bin/rmmod` `/bin/lsmod`
- `hello.mnx` / `hello.ko` (printk); `echo.*` registers `/dev/echo` via C-ABI `register_chrdev`
- Unload blocked while the device is open (`echotest` checks EBUSY)
- Kernel debug shell also has `insmod` / `rmmod` / `lsmod` (after `exit` from sh)
- **linuxkpi L0–L5 + virtio-net:** `virtio_blk.ko` → `/dev/vda`; **`virtio_net.ko`** ICMP ping 10.0.2.2. `make run` attaches virtio-blk-pci + virtio-net-pci (user net). qemu-connect is IDE-only. Plan: [docs/LINUXKPI.md](docs/LINUXKPI.md).

### Console
- VGA 80×25 text, PS/2 keyboard (US QWERTY)
- Userspace prompt **`$`**; kernel debug shell after sh exits

---

## Documentation

| Doc | Contents |
|-----|----------|
| **[docs/ROADMAP.md](docs/ROADMAP.md)** | Product goal (Linux desktop results) + phased plan (P1–P14) |
| **[docs/MM.md](docs/MM.md)** | Memory layout + phase checklist |
| **[docs/ABI.md](docs/ABI.md)** | Syscall calling convention, process model, FD rules |
| **[docs/SYSCALL_COMPARE.md](docs/SYSCALL_COMPARE.md)** | Linux x86_64 vs munux syscall coverage |
| **[docs/SMOKE_PREEMPT.md](docs/SMOKE_PREEMPT.md)** | IRQ preemption tests (`preempttest`) |
| **[docs/SMOKE_CLONE.md](docs/SMOKE_CLONE.md)** | `clone` / tid smoke |
| **[docs/SMOKE_SIGNAL.md](docs/SMOKE_SIGNAL.md)** | Signals + Ctrl-C |
| **[docs/SMOKE_FUTEX.md](docs/SMOKE_FUTEX.md)** | Futex join smoke |
| **[docs/SMOKE_VFS.md](docs/SMOKE_VFS.md)** | VFS mounts / fops / pipes (Phase 7) |
| **[docs/SMOKE_MODULE.md](docs/SMOKE_MODULE.md)** | Modules: insmod/rmmod/lsmod, hello + `/dev/echo` |
| **[docs/LINUXKPI.md](docs/LINUXKPI.md)** | Plan: Linux driver sources (linuxkpi) |
| **[docs/BUSYBOX_SUITE_REPORT.md](docs/BUSYBOX_SUITE_REPORT.md)** | Strict BusyBox regression suite |
| **[SMOKE.md](SMOKE.md)** | Manual smoke checklist |

---

## Quick start

```sh
make              # same as make run → run-iso (ISO + disk + QEMU)
make run          # alias for run-iso (Multiboot2 ELF64 needs GRUB, not -kernel)
make run-iso      # GRUB ISO (cdrom) + ext2 disk
make iso          # build kernel.iso only
make disk         # recreate build/disk.img
make help         # all targets
make size         # kernel / ISO size report
```

QEMU `-kernel` **cannot** load this Multiboot2 x86_64 ELF. Always boot via ISO (`make run` / `make run-iso` / `make debug-iso`).

### Useful targets

| Target | Description |
|--------|-------------|
| `build` | Release kernel → `build/kernel.bin` |
| `build_debug` | Debug symbols |
| `run` | Alias of `run-iso` |
| `run-iso` | GRUB ISO (cdrom) + ext2 disk |
| `iso` | Produce `build/kernel.iso` |
| `disk` | Recreate ext2 `build/disk.img` (+ rootfs tools) |
| `debug-iso` | Debug kernel + ISO + GDB stub (preferred) |
| `debug` / `debug-gdb` | `-kernel` + GDB — **may not boot** this Multiboot2 ELF |
| `size` | Print artifact sizes |
| `clean` / `fclean` / `re` | Cleanup / rebuild |

**IDE layout:** primary master (`index=0`) = ext2 disk; ISO uses `index=1` as cdrom.

**Headless automation:** qemu-connect MCP/CLI. Pass **this** tree’s `build/kernel.iso` + `build/disk.img` and prompt `$`. Default env `QEMU_CONNECT_MUNUX` may point at another checkout. There is **no** in-tree `scripts/busybox_suite.py`.

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
| `cmd \| cmd` | **Not parsed** by this shell (`|` is just argv). `pipe(2)` exists for programs that call it. |

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
├── docs/               # ABI, roadmap, syscall compare, suite reports
├── SMOKE.md
└── src/
    ├── kernel.rs
    ├── gdt/            # GDT + TSS
    ├── interrupts/     # IDT, PIC, exceptions, keyboard, timer
    ├── memory/         # PMM, paging (clone_mm), heap, Multiboot
    ├── process/        # PCB, fork, clone, sched, signals, futex, sys
    ├── module/         # loadable modules (export, MNX1, ELF ET_REL)
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
   - `$ insmod /lib/modules/hello.ko` · `$ rmmod hello`
   - `$ insmod /lib/modules/echo.ko` · `$ echotest` · `$ rmmod echo`
   - `munux> preempttest` (IRQ preemption A–G)
3. BusyBox probe notes: `docs/BUSYBOX_SUITE_REPORT.md` (2026-08-02 dump + 2026-08-07 overlay)  
4. Headless: `make iso disk`, then **qemu-connect** with this tree’s ISO/disk and prompt `$`

### Quick smoke after boot

```text
$ signaltest          # caught + parent ok
$ clonetest
$ futextest
$ forktest
$ insmod /lib/modules/hello.ko
$ lsmod
$ rmmod hello
$ insmod /lib/modules/echo.ko
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

**Modules (P8a–8c done; linuxkpi planned)**
- Formats today: **MNX1** and ELF64 **ET_REL** `.ko` with `munux_*` exports (NASM hello/echo)
- **Plan:** compile Linux driver **sources** — [docs/LINUXKPI.md](docs/LINUXKPI.md). Distro `.ko` binaries / vermagic still out of scope
- No `depmod` / signing / livepatch
- IDE/`hda` is **intentionally built-in** (root filesystem lives on that disk; a disk `.ko` would need initrd)
- Heap dual-map + PC32 trampolines (not Linux `vmalloc` + shared kernel PDPT)

**Process / MM**
- **No full Linux signal frame** (`siginfo` / `ucontext` / SA_NODEFER / RT signals)
- **No futex timeout / requeue / PI**; pthread mutex soak incomplete
- **No in-kernel preemption**; IRQ preempt gated under deep nest (depth ≥ 2 cooperative)
- **No higher-half kernel**; identity map + high heap for modules
- File `mmap` is **private snapshot** (no `MAP_SHARED` writeback / no COW)

**VFS / syscalls / HW**
- No `mount`/`umount` syscalls; no full dentry cache
- **No networking**, no SMP
- Subset of Linux syscalls; rest **`-ENOSYS`**
- VGA only (no serial console yet); US QWERTY only

See **[docs/ROADMAP.md](docs/ROADMAP.md)** (Phase 9 next) and **[docs/SMOKE_MODULE.md](docs/SMOKE_MODULE.md)**.

---

## License / acknowledgements

Licensing not fully specified in-tree. Descended from the **42** KFS track (bare-metal boot, interrupts, memory, minimal OS services). Continuing as **munux**.
