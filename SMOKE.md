# munux smoke checklist (x86_64)

Quick checks after `make run` / `make run-iso` / `make iso` (kernel + `build/disk.img`).

Prompt: userspace **`$`** after boot. Kernel debug shell appears only after `exit` from sh.

**Focused feature smokes (details):**  
[SMOKE_PREEMPT](docs/SMOKE_PREEMPT.md) · [SMOKE_CLONE](docs/SMOKE_CLONE.md) ·  
[SMOKE_SIGNAL](docs/SMOKE_SIGNAL.md) · [SMOKE_FUTEX](docs/SMOKE_FUTEX.md) ·  
[SMOKE_VFS](docs/SMOKE_VFS.md) · [SMOKE_MODULE](docs/SMOKE_MODULE.md)

---

## Boot

- [ ] QEMU starts without IDE unit conflict (`bus=0, unit=0`)
- [ ] VGA shows long mode / PMM / paging / IRQs / FS mount
- [ ] Handoff to userspace shell (prompt **`$`**)
- [ ] No immediate kernel panic on idle

---

## Foundation (process / threads / signals) — run after architecture work

From freestanding **`$`**:

- [ ] `signaltest` — prints `caught` then `parent ok`
- [ ] `clonetest` — child + parent ok
- [ ] `futextest` — child + parent ok (join via futex / clear_child_tid)
- [ ] `forktest` — still green
- [ ] `busybox true` — returns to `$`
- [ ] Optional Ctrl-C: `busybox sleep 30` then **Ctrl+C** → back to `$` (shell stays alive)

From kernel shell after `exit`:

- [ ] `preempttest` — `pass=7 fail=0` (A–G)

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
- [ ] `busybox cp t_smoke.txt t_smoke2.txt` (if `cp` works without rename)
- [ ] `busybox rm t_smoke.txt` (and cleanup)
- [ ] `busybox cat /proc/meminfo` (or similar) — proc readable when available
- [ ] Note: `busybox mv` / `ln` use VFS rename/link (P7d) — report if ENOSYS returns

---

## Modules (Phase 8) — needs `make disk`

From **`$`**:

- [ ] `ls /lib/modules` — `hello.mnx` `echo.mnx`
- [ ] `insmod /lib/modules/hello.mnx` then `lsmod` then `rmmod hello`
- [ ] `insmod /lib/modules/echo.mnx` then `ls /dev` shows `echo`
- [ ] `echotest` — `PASS` (read/write + EBUSY while open)
- [ ] `rmmod echo` then `ls /dev` has no `echo`
- [ ] `cat /proc/modules` matches `lsmod`

Kernel shell after `exit` also has `insmod`/`rmmod`/`lsmod` (bare `hello` can be builtin).

---

## Kernel debug shell (after userspace exit)

- [ ] `ps` — at least `kinit` (pid 1)
- [ ] `run sh` / `run init` — re-enter userspace
- [ ] `preempttest` — see foundation section
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
