# KFS smoke checklist

Quick manual checks after `make build` / `make run` (or `make run-iso`).

## Boot

- [ ] QEMU starts without "drive with bus=0, unit=0 exists"
- [ ] VGA shows boot banner (`KFS i686 kernel`)
- [ ] Lines mention: IDT gates, PMM frames, paging ON, heap, process init, ext2 mount (if disk present), `syscall: int 0x80 armed`
- [ ] Shell prompt `kfs> ` appears

## Core commands

- [ ] `help` — list includes `user`, fs, pmm, heap
- [ ] `about` — GDT 8 entries, TSS selector `0x38`, paging/heap stats
- [ ] `gdt` — 8 rows; index 7 is `tss` with access `0x89` (TSS-32)
- [ ] `idt` — present gates include exceptions + IRQ0 + IRQ1 + 0x80
- [ ] `pmm` / `vmm` / `heap` — info and optional `test` subcommands pass
- [ ] `ps` — at least init (pid 1)

## Filesystem (needs `build/disk.img` via `make disk` / `make run`)

- [ ] `ls` — shows `hello.txt`, `docs/`
- [ ] `cat hello.txt` — "Hello from KFS ext2!"
- [ ] `pwd` / `cd docs` / `pwd` / `cd ..`
- [ ] `mkdir testdir` then `ls` shows it
- [ ] `touch foo.txt` then `rm foo.txt`
- [ ] `rmdir testdir`

## User mode / syscalls

- [ ] `user` (or `usermode`) prints:
  - `user: entering ring 3 @ 0x400000 …`
  - `Hello from ring 3 user mode via int 0x80!`
  - `user: returned to kernel (exit)`
- [ ] Shell still accepts input after `user` (keyboard IRQs still work)

## Size (soft subject limit ~10 MiB)

```sh
make size
```

- [ ] `build/kernel.bin` is well under 10 MiB (typically a few hundred KiB–low MiB with debug)
- [ ] Scrollback BSS is modest (48 lines × 6 screens — see Makefile note)

## Optional debug

```sh
make debug
# (gdb) break kmain
# (gdb) continue
```

- [ ] GDB attaches without auto-load safe-path spam (`-nx -x gdb/kfs.gdb`)

## ELF exec

- [ ] `ls /bin` or `ls bin` shows `hello` (depends on path form)
- [ ] `run /bin/hello` prints `Hello from ELF userland!` and returns to `kfs>`
- [ ] `user` still runs the built-in demo
