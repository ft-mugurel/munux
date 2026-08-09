# Phase 4 smoke: clone / threads (first slice)

## What is implemented

| Piece | Notes |
|-------|--------|
| `gettid` (186) | Unique task id (`pid` field) |
| `getpid` (39) | Returns **tgid** |
| `clone` (56) | `CLONE_VM`, `CLONE_FILES`, `CLONE_THREAD`, settid, stack, TLS |
| `clone3` (435) | `struct clone_args` (flags…tls); stack is low + `stack_size`; child keeps parent GPRs (`rdx`/`r8` for glibc) |
| `set_tid_address` | Stores `clear_child_tid`; returns tid |
| Shared mm free | `free_mm` only when last task with that CR3 exits |
| Shared FDs | `CLONE_FILES` refcount on FD tables |
| `exit_group` (231) | Kills whole thread group; one zombie for wait |

## Quick test (qemu-connect MCP)

```text
$ clonetest
clonetest: child ok
clonetest: parent ok
$ tlsclone
tlsclone: child FS OK
tlsclone: clone3 child OK
tlsclone: ALL PASS
$ clonec
clonec: child ran
clonec: ALL PASS
$ forktest
$ busybox true
$ exit
munux> run clonetest
```

Expect both parent and child banners; no panic. `tlsclone` checks `CLONE_SETTLS` (`[fs:0]`) plus clone3 trampoline regs. `clonec` is **glibc `clone()`** (fn on the new stack; child keeps libc TLS).

## Related (Phase 6 — done in first slice)

- Clear TID word + futex wake on exit: see [SMOKE_FUTEX.md](SMOKE_FUTEX.md)

## Not yet

- Real `CLONE_FS` (shared cwd object)
- Full glibc `pthread_create` / nptl soak (`/bin/pthreadtest` still `#PF RIP=0`)
