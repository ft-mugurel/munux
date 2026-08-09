# Smoke: Phase 8 modules (insmod / rmmod / lsmod)

**Status:** Phase **8a–8c** done (2026-08-07) — MNX1 + ELF64 ET_REL `.ko`,
Linux syscalls, userspace tools, `/dev/echo` + unload refcount.

## Why kernel shell *and* normal shell?

On Linux, `insmod`/`rmmod`/`lsmod` are **userspace** programs that call:

| Tool | Syscall | Number |
|------|---------|--------|
| `insmod` | `finit_module` (or `init_module`) | 313 / 175 |
| `rmmod` | `delete_module` | 176 |
| `lsmod` | read `/proc/modules` | open/read |

munux matches that: kernel implements the syscalls + `/proc/modules`; `/bin/insmod` etc. live on the disk.

## Userspace shell (`$` prompt) — preferred

```text
$ ls /lib/modules          # hello.ko hello.mnx hello_c.ko echo.ko echo.mnx
$ lsmod
$ insmod /lib/modules/hello_c.ko   # linuxkpi gcc: "hello_c: linuxkpi module loaded"
$ rmmod hello_c
$ insmod /lib/modules/echo_c.ko    # linuxkpi misc: /dev/echo (not together with echo.ko)
$ echotest                         # PASS + EBUSY while open
$ rmmod echo
$ insmod /lib/modules/irqtest.ko   # linuxkpi: "irqtest: got IRQ0 (timer) PASS"
$ rmmod irqtest
$ insmod /lib/modules/vprobe.ko    # PCI: Intel/VGA (+ virtio if QEMU has virtio-blk-pci)
$ rmmod vprobe
# virtio-blk needs QEMU -device virtio-blk-pci (make run). qemu-connect is IDE-only.
$ insmod /lib/modules/virtio_blk.ko
$ ls /dev                          # vda
$ vdatest                          # PASS if vda.img starts with VIRTIOBLK
# virtio-net needs -netdev user -device virtio-net-pci (make run)
$ insmod /lib/modules/virtio_net.ko  # "virtio_net: ping 10.0.2.2 PASS"
$ rmmod virtio_net
$ insmod /lib/modules/hello.ko
$ lsmod
$ rmmod hello
$ insmod /lib/modules/hello.mnx    # still supported
$ rmmod hello
$ cat /proc/modules
```

Expected:

1. Empty `lsmod` / empty `/proc/modules` at first.
2. `insmod …/hello.ko` → `hello: module loaded (elf)` then `module: loaded hello`.
3. `lsmod` → line like `hello 368 0 - Live 0x0`.
4. `rmmod hello` → `hello: module unloaded (elf)`.
5. `.mnx` path still prints `(mnx)` and unloads cleanly.

## Kernel debug shell (after `exit`)

Still works for bring-up without userspace tools:

```text
munux> lsmod
munux> insmod hello
munux> rmmod hello
```

## Build

```bash
make build iso disk
```

Produces `/bin/insmod`, `/bin/rmmod`, `/bin/lsmod` and
`/lib/modules/{hello,echo}.{ko,mnx}`.

## Format note

| Suffix | Format | Source |
|--------|--------|--------|
| `.mnx` | munux **MNX1** (abs64 relocs) | `modules/hello.asm` |
| `.ko` | ELF64 **ET_REL** (not mainline vermagic) | `modules/hello.ko.asm` |
| `hello_c.ko` | gcc linuxkpi (`include/linux/*.h`) | `modules/linux/hello.c` |
| `echo_c.ko` | gcc linuxkpi `misc_register` + `file_operations` | `modules/linux/echo.c` |
| `irqtest.ko` | gcc linuxkpi `request_irq(0, SHARED)` + completion | `modules/linux/irqtest.c` |
| `vprobe.ko` | gcc linuxkpi `pci_register_driver` | `modules/linux/vprobe.c` |
| `virtio_blk.ko` | modern virtio-pci blk → `/dev/vda` | `modules/linux/virtio_blk.c` |
| `virtio_net.ko` | modern virtio-pci net + ICMP ping 10.0.2.2 | `modules/linux/virtio_net.c` |

Bare `insmod hello` tries `.ko` then `.mnx` then builtin `hello`.
Userspace `insmod` uses `finit_module`; ELF name comes from `.modinfo name=`.

See `src/module/mnx.rs`, `src/module/elfrel.rs`.

## Chardev module (`echo.mnx`) — Phase 8b

```text
$ insmod /lib/modules/echo.ko     # or echo.mnx
$ ls /dev
$ echotest
$ lsmod
$ rmmod echo
```

Expected:

1. `echo: module loaded (/dev/echo)`
2. `/dev` lists `echo` next to `null`/`zero`/`hda`
3. `echotest: PASS` — write/read round-trip, and `delete_module` returns **EBUSY** while the fd is still open
4. After `echotest` closes, `rmmod echo` succeeds and `/dev/echo` disappears

## Not yet

| Missing | Notes |
|---------|--------|
| Mainline Linux **vermagic / ksymtab / GPL** | Our `.ko` is ET_REL + munux exports only |
| Compile Linux **driver `.c`** (linuxkpi) | **Planned** — [LINUXKPI.md](LINUXKPI.md); not P8 leftover |
| `depmod`, signing, **distro** `.ko` binaries | Still out of scope |
| IDE as a `.ko` on ext2 | **Not a bug / not unfinished P8.** Root is on `hda`; `insmod` needs that disk. IDE stays **built-in**. Initrd + optional ATA module is later boot work. |
| Shared kernel PDPT / `vmalloc` | Heap dual-map + trampolines for PC32 |
| Module-loaded filesystems | VFS can register; no example FS module yet |

**Phase 8 is complete.** Next epic is Phase 9 (see ROADMAP). Do not restart P8 for IDE-as-`.ko`.

## Sources

- Kernel: `src/module/` (`mod.rs`, `export.rs`, `mnx.rs`, `elfrel.rs`)
- Modules: `modules/hello.asm`, `echo.asm`, `hello.ko.asm`, `echo.ko.asm`
- Userspace: `userland/insmod.asm`, `rmmod.asm`, `lsmod.asm`, `echotest.asm`
