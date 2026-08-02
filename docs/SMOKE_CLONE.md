# Phase 4 smoke: clone / threads (first slice)

## What is implemented

| Piece | Notes |
|-------|--------|
| `gettid` (186) | Unique task id (`pid` field) |
| `getpid` (39) | Returns **tgid** |
| `clone` (56) | `CLONE_VM`, `CLONE_THREAD`, settid flags, stack, `CLONE_SETTLS` |
| `set_tid_address` | Stores `clear_child_tid`; returns tid |
| Shared mm free | `free_mm` only when last task with that CR3 exits |

## Quick test (qemu-connect MCP)

```text
$ clonetest
clonetest: child ok
clonetest: parent ok
$ forktest
$ busybox true
$ exit
munux> run clonetest
```

Expect both parent and child banners; no panic.

## Not yet (later Phase 4 / 6)

- Refcounted shared FD tables (`CLONE_FILES` real share)
- `exit_group` kills whole thread group
- Clear TID word + futex wake on exit (pthread join)
