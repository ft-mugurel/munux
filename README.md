# munux

**munux** is a freestanding operating-system kernel written in **Rust** and **NASM**. It boots via **Multiboot/GRUB**, runs under **QEMU**, and aims for long-term **Linux-compatible** userspace (syscalls, processes, VFS) while staying small and readable.

Started as a **42 KFS** learning kernel; the project now continues independently as **munux**.

| Branch | Role |
|--------|------|
| **`main`** | Active development (x86_64 port in progress) |
| **`32bit`** | Frozen **i686** snapshot (full Multiboot kernel as of the rename) |

Repository: [github.com/ft-mugurel/munux](https://github.com/ft-mugurel/munux)

**ABI:** syscall numbers, FDs, and init policy are documented in [`docs/ABI.md`](docs/ABI.md) (v0.1).

---

## Current capabilities (i686 baseline on `32bit` / shared tree)

### Boot & build
- Multiboot 1 header + assembly entry (`_start`), stack at `stack_top`
- Custom freestanding Rust target (`#![no_std]`, soft-float, panic = abort)
- Kernel linked at **1 MiB**; GRUB ISO + QEMU targets
- IDE disk image (`build/disk.img`) with a small **ext2** rootfs

### CPU / interrupts
- **GDT** at fixed address **`0x800`** (8 entries): null, kcode, kdata, kstack, ucode, udata, ustack, **TSS**
- **TSS** (`ltr 0x38`) — ring-3 → ring-0 stack switch
- **IDT** — exceptions, **IRQ0** (PIT), **IRQ1** (keyboard), **int 0x80** (syscalls, DPL=3)
- 8259 PIC remapped; kernel signal / callback helpers

### Memory
- Multiboot memory map → **PMM** (frame bitmap)
- Identity-mapped **paging** (CR0.PG)
- Kernel **heap** (`kmalloc` freelist)

### Processes
- PCB table, fork/wait/kill/signal/getuid, simple in-kernel sockets
- Timer-driven per-process signal delivery

### Filesystem
- ATA PIO **IDE** (primary master) + **ext2** read/write
- Shell: `ls`, `cat`, `pwd`, `cd`, `mkdir`, `touch`, `rm`, `rmdir`

### User mode
- Ring-3 demo at `0x00400000` (code) / `0x00500000` (stack)
- **ELF32 loader** — `run /bin/hello` loads `ET_EXEC` from ext2
- Syscalls via **`int 0x80`**: EXIT, WRITE, READ, OPEN, CLOSE, GETPID, GETUID, FORK, WAIT, KILL, SIGNAL
- Shell: **`user`** (embedded demo) · **`run` / `exec`** (ELF)

### Console
- VGA 80×25, 6 virtual screens (F1–F6), modest scrollback
- PS/2 keyboard (US QWERTY), interactive shell (`kfs>` prompt in the i686 tree)

---

## Roadmap

1. **x86_64 port** on `main` (long mode, 4-level paging, `syscall`)
2. Linux-shaped **FD table**, VFS, and syscall ABI
3. Per-process address spaces, `execve`, static musl / BusyBox
4. Broader POSIX surface toward real-world use

---

## Quick start

```sh
make              # build ISO + disk + boot (run-iso)
make run          # -kernel + IDE disk (faster iteration)
make help         # all targets
make size         # kernel / ISO size report
```

### Useful targets

| Target | Description |
|--------|-------------|
| `build` | Release kernel → `build/kernel.bin` |
| `build_debug` | Debug symbols |
| `run` | QEMU `-kernel` + `disk.img` on IDE index 0 |
| `run-iso` | GRUB ISO (index 1) + disk (index 0) |
| `disk` | Recreate 32 MiB ext2 `build/disk.img` |
| `debug` / `debug-gdb` | QEMU GDB stub + `gdb/kfs.gdb` |
| `size` | Print artifact sizes |
| `clean` / `fclean` / `re` | Cleanup / rebuild |

**IDE layout:** primary master (`index=0`) = ext2 disk; ISO uses `index=1` as cdrom. Do not put two drives on unit 0.

---

## Shell cheat sheet

| Command | Description |
|---------|-------------|
| `help [cmd]` | Hierarchical help |
| `about` | Kernel / GDT / memory summary |
| `gdt` / `idt` / `regs` / `stack` / `mem` | Debug dumps |
| `pmm` / `vmm` / `heap` | Memory subsystems (+ `test`) |
| `ps` / `fork` / `wait` / `kill` / `signal` | Processes |
| `ls` `cat` `pwd` `cd` `mkdir` `touch` `rm` `rmdir` | ext2 |
| **`user`** | Enter ring 3, demo syscalls, return |
| `run` / `exec` | Load ELF from disk into ring 3 |
| `reboot` / `halt` / `panic` / `fault` | Machine control / tests |

Keys: **F1–F6** screens · **Shift+Up/Down** scroll · **Ctrl+Alt+Del** poweroff

---

## Boot flow (i686)

```text
QEMU → GRUB (or -kernel) → _start (ESP=stack_top, save Multiboot)
  → load_gdt() → init_tss()
  → init_idt() → exceptions → PIC → keyboard → timer
  → sti → VGA screens
  → PMM → paging → heap → processes → fs → init_syscalls (int 0x80)
  → shell loop (process_signals + hlt)
```

### GDT (at `0x800`)

| Index | Segment | Selector |
|------:|---------|----------|
| 0 | Null | `0x00` |
| 1 | Kernel code | `0x08` |
| 2 | Kernel data | `0x10` |
| 3 | Kernel stack | `0x18` |
| 4 | User code | `0x23` (RPL3) |
| 5 | User data | `0x2B` |
| 6 | User stack | `0x33` |
| 7 | TSS | `0x38` |

### Syscall ABI (`int 0x80`, current tree)

| EAX | Name | Args |
|----:|------|------|
| 0 | EXIT | EBX=status |
| 1 | WRITE | EBX=fd, ECX=buf, EDX=len |
| 2 | READ | (stub) |
| 3 | OPEN | EBX=path |
| 5 | GETPID | — |
| 6 | GETUID | — |
| … | FORK/WAIT/KILL/SIGNAL | see `src/syscalls/mod.rs` |

---

## Project layout

```text
.
├── Makefile
├── multiboot/          # header, exceptions, timer, syscall.asm
├── linker/linker.ld
├── grub/grub.cfg
├── gdb/kfs.gdb
├── SMOKE.md            # manual test checklist
└── src/
    ├── kernel.rs
    ├── gdt/            # GDT + TSS
    ├── interrupts/     # IDT, PIC, exceptions, keyboard, timer, signals
    ├── memory/         # PMM, paging, heap, Multiboot parse
    ├── process/        # PCB, fork/wait, sockets
    ├── drivers/ide.rs
    ├── fs/             # ext2 + VFS path
    ├── syscalls/       # int 0x80 + ring-3 demo
    ├── shell/
    └── vga/
```

---

## Requirements

| Tool | Purpose |
|------|---------|
| Rust nightly (`rust-toolchain.toml`) | `build-std` / freestanding |
| nasm, ld | ASM + final link |
| grub-mkrescue + appropriate GRUB modules | ISO |
| qemu-system-i386 (32-bit baseline) / qemu-system-x86_64 (port) | Emulation |
| e2fsprogs (`mkfs.ext2`) | Disk image |

```sh
# illustrative
rustup toolchain install nightly
# nasm binutils grub-pc-bin xorriso qemu-system-x86 e2fsprogs
```

---

## Smoke test

See **[SMOKE.md](SMOKE.md)** for a step-by-step manual checklist (boot, FS, `user`, size).

---

## Limitations

- Single shared address space (no per-process page tables yet)
- `read` / full file-descriptor table are stubs
- No preemptive multi-process scheduling beyond cooperative PCBs
- Custom syscall numbers (not Linux ABI yet)
- VGA only (no serial console yet)
- US QWERTY only

---

## License / acknowledgements

Licensing not fully specified in-tree. Descended from the **42** KFS track (bare-metal boot, interrupts, memory, minimal OS services). Continuing as **munux**.
