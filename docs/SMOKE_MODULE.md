# Smoke: Phase 8 modules (insmod / rmmod / lsmod)

**Status:** Phase 8a+b **practical done** (2026-08-07) — MNX1 loader, Linux
syscalls, userspace tools, `/dev/echo` + unload refcount.

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
$ lsmod
$ insmod /lib/modules/hello.mnx
$ lsmod
$ rmmod hello
$ lsmod
$ cat /proc/modules
```

Expected:

1. Empty `lsmod` / empty `/proc/modules` at first.
2. `insmod` → kernel prints `hello: module loaded (mnx)`.
3. `lsmod` → line like `hello 100 0 - Live 0x0`.
4. `rmmod hello` → unload; list empty again.

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

Produces `/bin/insmod`, `/bin/rmmod`, `/bin/lsmod` and `/lib/modules/hello.mnx`.

## Format note

`.mnx` is **munux’s** module container (MNX1), not a mainline Linux `.ko`.
See `src/module/mnx.rs` and `modules/hello.asm`.

## Chardev module (`echo.mnx`) — Phase 8b

```text
$ insmod /lib/modules/echo.mnx
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

## Not yet (do **not** restart P8 unless you want these)

| Missing | Notes |
|---------|--------|
| ELF **ET_REL** / mainline **`.ko`** | MNX1 is munux’s own container (`src/module/mnx.rs`) |
| Vermagic / param string / GPL symbols | `init_module` ignores `uargs` |
| `depmod`, dependencies, signing, livepatch | Explicit non-goals |
| IDE as a loadable module | Still built-in `hda` |
| Shared kernel PDPT / `vmalloc` | Heap dual-map + `map_code_into_current` workaround |
| Module-loaded filesystems | VFS can register; no example FS module yet |

**Next epic is Phase 9** (see ROADMAP handoff): file-backed mmap, symlink/statx,
epoll/select — not more module format work unless needed.

## Sources

- Kernel: `src/module/` (`mod.rs`, `export.rs`, `mnx.rs`)
- Modules: `modules/hello.asm`, `modules/echo.asm`
- Userspace: `userland/insmod.asm`, `rmmod.asm`, `lsmod.asm`, `echotest.asm`
