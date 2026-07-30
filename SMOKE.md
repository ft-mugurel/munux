# munux smoke checklist (x86_64)

Quick checks after `make run` / `make run-iso` / `make iso` (kernel + `build/disk.img`).

Prompt: userspace **`$`** after boot. Kernel debug shell appears only after `exit` from sh.

---

## Boot

- [ ] QEMU starts without IDE unit conflict (`bus=0, unit=0`)
- [ ] VGA shows long mode / PMM / paging / IRQs / FS mount
- [ ] Handoff to userspace shell (prompt **`$`**)
- [ ] No immediate kernel panic on idle

---

## Freestanding `/bin/sh`

- [ ] `help` — builtins listed
- [ ] `pwd` / `cd` / `pwd`
- [ ] `ls` — lists root (`.`, `..`, `bin`, `proc`, …)
- [ ] `cat hello.txt` (or similar on disk)
- [ ] Unknown path → userland error, not kernel panic
- [ ] Backspace / basic line edit
- [ ] `exit` → returns to kernel shell (or controlled exit path)

---

## BusyBox (static on rootfs)

Requires `/bin/busybox` on `disk.img`.

- [ ] `busybox true` — returns to `$`
- [ ] `busybox uname` — prints `munux` (or similar)
- [ ] `busybox ls` / `busybox ls bin`
- [ ] `busybox echo hi`
- [ ] `busybox touch t_smoke.txt`
- [ ] `busybox cp t_smoke.txt t_smoke2.txt`
- [ ] `busybox mv t_smoke2.txt t_moved.txt` — **no ENOSYS**
- [ ] `busybox ls t_moved.txt` — exists; `t_smoke2.txt` gone
- [ ] `busybox rm t_moved.txt t_smoke.txt`
- [ ] `busybox free` — Mem: line with totals
- [ ] `busybox cat /proc/meminfo` (or `busybox free`) — proc readable
- [ ] `busybox sh` — interactive ash starts (`/ #` or similar)
- [ ] Inside ash: `echo hi` then `ls` then another command — **no panic (RIP=0)**
- [ ] Inside ash: `exit` back to freestanding `$` if applicable

---

## Kernel debug shell (after userspace exit)

- [ ] `ps` — at least `kinit` (pid 1)
- [ ] `run sh` / `run init` — re-enter userspace
- [ ] `help` / `about`
- [ ] Kernel `ls` / `cat` helpers still work

---

## Regression automation

```sh
make iso
# headless: qemu-connect or scripts/busybox_suite.py
python3 scripts/busybox_suite.py   # if environment configured
```

Update **[docs/BUSYBOX_SUITE_REPORT.md](docs/BUSYBOX_SUITE_REPORT.md)** when the suite is re-run.

---

## Optional

```sh
make size
make debug   # GDB stub + gdb/kfs.gdb
```

---

## Known failure modes (do not “fix” in smoke)

| Symptom | Likely cause |
|---------|----------------|
| `ENOSYS n=…` | Unimplemented syscall (see SYSCALL_COMPARE) |
| Panic after fork/exec under ash | Shared AS / TLS / nest — check ROADMAP P1 |
| `find .` hang | Known suite hang (readdir loop / missing path) |
| Network applets | No sockets yet |
