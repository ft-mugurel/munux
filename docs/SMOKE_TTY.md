# P11a smoke: session / pgrp / console termios

## What is implemented

| Piece | Notes |
|-------|--------|
| `getpgrp` (111) / `getpgid` (121) / `getsid` (124) | Per-task `pgid` / `sid` (fork inherits) |
| `setpgid` (109) | Self or children, same session; new group or existing |
| `setsid` (112) | New session if not already a pgrp leader; drop ctty |
| Console ioctl | `TCGETS`/`TCGETS2`, `TCSETS*`, `TIOCGWINSZ` (80×25), `TIOCGPGRP`/`TIOCSPGRP`, `TIOCSCTTY`/`TIOCNOTTY`/`TIOCGSID` |
| `kill(-pgid)` / `kill(0)` | Process-group directed |

## Quick test (qemu-connect)

```text
$ jobtest
jobtest: child session OK
jobtest: ALL PASS
$ hello_dyn
hello_dyn: ALL PASS
```

`jobtest` is a glibc binary: `isatty`, `tcgetattr`, winsize, `setpgid(0,0)`, child `setsid` + `TIOCSCTTY`.

## Not yet

- `/dev/ptmx` + `/dev/pts/N` pair (P11b)
- Real n_tty line discipline (canonical editing in kernel)
- `SIGTTOU`/`SIGTTIN` / stopped jobs
