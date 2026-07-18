# munux smoke checklist (x86_64)

Quick checks after `make run` / `make run-iso` (builds ISO + `build/disk.img`).

## Boot (U8)

- [ ] QEMU starts without "drive with bus=0, unit=0 exists"
- [ ] VGA: `munux x86_64`, long mode, PMM, paging, heap, IRQs, FDs, process, FS
- [ ] Line: `U8: handoff → /bin/sh`
- [ ] Userspace prompt `$` (not `munux>` first)
- [ ] Banner: `munux sh (U7)...`

## Userspace `/bin/sh`

- [ ] `help` — builtins + edit note
- [ ] `hello` — ELF message
- [ ] `cat` / `cat hello.txt` / `cat docs/readme.txt`
- [ ] `cat /no/such` → `cat: cannot open file` (not `sh: exec failed`)
- [ ] `ls` / `pwd` / `cd docs` / `pwd`
- [ ] Backspace / `clear`
- [ ] `exit` → `U8: /bin/sh exited` then `munux>` kernel shell

## Kernel debug shell (after exit)

- [ ] `ps` — at least `kinit` (pid 1)
- [ ] `run sh` or `run init` — re-enter userspace shell
- [ ] `run shtest` — scripted smoke (cat/ls/pwd/exit)
- [ ] `ls` / `cat hello.txt` (kernel FS commands)
- [ ] `help` / `about`

## Optional

```sh
make size
make debug   # GDB stub + gdb/kfs.gdb
```
