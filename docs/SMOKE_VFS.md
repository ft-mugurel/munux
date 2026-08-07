# Phase 7 smoke: VFS (complete practical)

## Implemented (7a–7d)

| Piece | Notes |
|-------|--------|
| `FileOperations` | console, ext2, ramfs, null, zero, proc, vdir, pipe, blk |
| Mounts | `/` ext2, `/ram` ramfs, `/proc` proc — visible in `ls /` |
| chrdev / blkdev | `/dev/null`, `/dev/zero`, `/dev/hda`; IDE as `hda` |
| Path mutations | mkdir/unlink/rmdir/rename/link via `vops` → ext2 |
| Pipes | `pipe`/`pipe2`, `dup`/`dup2` (cooperative schedule) |

## Userspace matrix

| Command | Expected |
|---------|----------|
| `ls` / `ls /` | `proc` `dev` `ram` + ext2 names |
| `ls -l /` | same names; `stat` works on virtual mounts (not ENOENT) |
| `ls /proc` | meminfo mounts version uptime self **modules** |
| `ls /dev` | null zero **hda** (+ **echo** only after `insmod echo.mnx`) |
| `ls /ram` | hello |
| `cat /proc/meminfo` etc. | synthetic text |
| `cat /ram/hello` | ramfs says hi |
| `cat /dev/null` | empty |
| `mkdir t` / `rmdir t` | via vops |
| BusyBox `mv` / `ln` | **PASS** (qemu-connect 2026-08-07) — `rename`/`link` |
| `cat /proc/mounts` | `/dev/hda / ext2`, `ramfs /ram`, `proc /proc`, `devtmpfs /dev` |

**Known:** `cat /dev/zero` never ends (Linux-like). Prefer bounded reads.

**Pipes vs shell:** `pipe`/`pipe2`/`dup`/`dup2` are kernel syscalls. Freestanding `/bin/sh` does **not** parse `|` (it execs `echo` with extra argv). Use a userspace shell that calls `pipe(2)` (BusyBox ash) to exercise pipelines.

## Phase 7 exit

Practical criteria met for starting **P8 modules**. Full dentry cache and
`mount(2)` remain optional polish.
