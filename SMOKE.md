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

## Foundation (process / threads / signals) — spine smokes; desktop is later

From freestanding **`$`**:

- [ ] `signaltest` — prints `caught` then `parent ok`
- [ ] `clonetest` — child + parent ok
- [ ] `futextest` — child + parent ok (join via futex / clear_child_tid)
- [ ] `forktest` — still green
- [ ] `mmaptest` — A–E + `ALL PASS` (run twice; also `/bin/mmaptest`)
- [ ] `polltest` — A–F + `ALL PASS` (run twice; also `/bin/polltest`)
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
- [ ] `busybox ls -l /` — `proc`/`dev`/`ram` are dirs, not “No such file”
- [ ] `busybox echo hi`
- [ ] `busybox touch t_smoke.txt`
- [ ] `busybox cp t_smoke.txt t_smoke2.txt`
- [ ] `busybox mv t_smoke2.txt t_moved.txt` — **expected PASS** (`rename` 82)
- [ ] `busybox ln t_smoke.txt t_link.txt` — **expected PASS** (`link` 86)
- [ ] `busybox rm t_smoke.txt t_moved.txt t_link.txt` (cleanup)
- [ ] `busybox cat /proc/meminfo` — proc readable
- [ ] `busybox ln -s hello.txt t_link` then `busybox readlink t_link` → `hello.txt`
- [ ] `busybox cat t_link` follows the symlink
- [ ] Freestanding `sh` does **not** parse `|` (not a kernel ENOSYS)

---

## Modules (Phase 8) — needs `make disk`

From **`$`**:

- [ ] `ls /lib/modules` — `hello.ko` `hello.mnx` `echo.ko` `echo.mnx`
- [ ] `insmod /lib/modules/hello.ko` then `lsmod` then `rmmod hello` (`(elf)` messages)
- [ ] `insmod /lib/modules/hello.mnx` still works (`(mnx)`)
- [ ] `insmod /lib/modules/echo.ko` then `ls /dev` shows `echo`
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

There is **no** in-tree `scripts/busybox_suite.py`. Use **qemu-connect** against **this** tree
(the tool’s default `QEMU_CONNECT_MUNUX` may point at another checkout):

```sh
make iso disk
# MCP / CLI: pass iso + disk explicitly
#   iso  = build/kernel.iso
#   disk = build/disk.img
#   prompt = $
```

Update **[docs/BUSYBOX_SUITE_REPORT.md](docs/BUSYBOX_SUITE_REPORT.md)** when a full suite is re-run.

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
| Panic after fork/exec under ash | Shared AS / TLS / nest — check ROADMAP P1 (private mm is done; re-check nest) |
| `find .` hang | Known suite hang (readdir loop / missing path) — still open |
| Freestanding `cmd \| cmd` prints `\|` literally | `/bin/sh` has no pipeline parser; `pipe(2)` exists |
| Network applets | No sockets yet |
