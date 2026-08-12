# P11 smoke: session / pgrp / console termios / PTY

## What is implemented

| Piece | Notes |
|-------|--------|
| `getpgrp` (111) / `getpgid` (121) / `getsid` (124) | Per-task `pgid` / `sid` (fork inherits) |
| `setpgid` (109) | Self or children, same session; new group or existing |
| `setsid` (112) | New session if not already a pgrp leader; drop ctty |
| Console ioctl | `TCGETS`/`TCGETS2`, `TCSETS*`, `TIOCGWINSZ` (80×25), `TIOCGPGRP`/`TIOCSPGRP`, `TIOCSCTTY`/`TIOCNOTTY`/`TIOCGSID` |
| `kill(-pgid)` / `kill(0)` | Process-group directed |
| `/dev/ptmx` | Open allocates a pair; `TIOCGPTN` / `TIOCSPTLCK` |
| `/dev/pts/N` | Slave tty (termios, `TIOCSCTTY`); byte rings to master |
| n_tty (P11c) | Master input: `ICANON` line/erase, `ECHO`, `ISIG` (`^C`/`^\`) |

## Quick test (qemu-connect)

```text
$ jobtest
jobtest: child session OK
jobtest: ALL PASS
$ hello_dyn
hello_dyn: ALL PASS
```

`jobtest` is a glibc binary: `isatty`, `tcgetattr`, winsize, `setpgid(0,0)`, child `setsid` + `TIOCSCTTY`.

```text
$ ptytest
ptytest: ALL PASS
```

`ptytest` opens `/dev/ptmx`, unlocks, forks a child on `/dev/pts/N` (`setsid` + `TIOCSCTTY`), turns **`ICANON` off**, writes one byte on the master, reads `PTY-CHILD-OK`.

```text
$ n_ttytest
n_ttytest: ALL PASS
```

`n_ttytest`: master writes `ab` + DEL + `c` + NL → slave reads `ac\n`; then a child `TIOCSCTTY` + write `^C` on the master dies with SIGINT.

## Not yet

- Full n_tty (IXON, reprint, UTF-8 erase)
- `SIGTTOU`/`SIGTTIN` / stopped jobs
- Unlimited PTY count (pool is 4)
- Console keyboard is not yet fed through the same cook path
