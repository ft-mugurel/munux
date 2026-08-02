# Phase 5 smoke: signals (complete practical slice)

## Implemented

| Piece | Notes |
|-------|--------|
| `kill` (62) | Process-directed; default terminate |
| `tkill` (200) / `tgkill` (234) | Thread-directed |
| `rt_sigaction` (13) | Store handler / SIG_IGN / SIG_DFL |
| `rt_sigprocmask` (14) | Block / unblock / setmask (64-bit) |
| `rt_sigreturn` (15) | Restore context after handler |
| Default actions | SIGTERM/SIGINT/SIGKILL/… → terminate group |
| User handlers | Frame on user stack + kernel restorer; inject into Ready traps |
| `/bin/sh` | SIG_IGN for SIGINT/SIGQUIT at startup |

## Test

```text
$ signaltest
signaltest: caught
signaltest: parent ok
$ forktest
$ clonetest
$ futextest
$ busybox true
```

## Ctrl-C (TTY → SIGINT)

| Piece | Status |
|-------|--------|
| Keyboard Ctrl+C | pending flag (no teardown in keyboard IRQ) |
| Target | **prefer current user tgid** (job), else last console reader |
| Shell at `$` | ignores SIGINT (stays alive) |
| Job running | SIGINT terminates job; shell returns to prompt |

Manual: `$ busybox sleep 60` then Ctrl+C → `$` returns; shell itself is not killed.
