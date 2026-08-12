# munux ABI (working specification)

This document freezes conventions for userspace and the kernel.  
**Change only with a deliberate version bump.**

| Field | Value |
|-------|--------|
| **Status** | **v0.3.12** — P11c n_tty cook on PTY |
| **Arch** | **x86_64** only on `main` |
| **Goal** | Linux userspace (static musl today, glibc + a **desktop** later) uses the **same numbers, structs, and register ABI** as Linux; missing calls return **`-ENOSYS`**. Internals may differ; **results must match**. |

Product direction (install and use a Linux desktop): **[ROADMAP.md](ROADMAP.md)**.  
Full Linux vs munux matrix: **[SYSCALL_COMPARE.md](SYSCALL_COMPARE.md)**.

---

## 1. Calling convention (`syscall`)

Same as Linux x86_64:

| Item | Value |
|------|--------|
| Instruction | `syscall` / return via `sysret` |
| Number | `rax` |
| Args | `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` |
| Return | `rax` (or `-errno` as two’s complement `u64`) |
| Clobbered | `rcx` (RIP), `r11` (RFLAGS) |
| Kernel entry | `LSTAR` → `syscall_entry` (NASM); TLS MSRs cleared in kernel, restored on return |

**Not used on x86_64 `main`:** `int 0x80` (that was the i686 / early path).

---

## 2. Syscall numbers (implemented)

Reference: Linux `arch/x86/entry/syscalls/syscall_64.tbl`.  
Source of truth: `src/syscalls/mod.rs` dispatch `match` (not merely `num` constants).  
**101** numbers are dispatched (quality varies: full / partial / stub). Guest `uname` strings are still `sysname=munux`, `release=0.2.0`, `version=munux 0.2 x86_64` (independent of this ABI doc version).

| # | Linux name | munux status |
|--:|------------|--------------|
| 0 | `read` | done (stdin, files) |
| 1 | `write` | done (console, files) |
| 2 | `open` | done (`O_CREAT` / `O_TRUNC` / R/W) |
| 3 | `close` | done |
| 4 | `stat` | done |
| 5 | `fstat` | done |
| 6 | `lstat` | done (does **not** follow last symlink) |
| 7 | `poll` | done (P9c; ms timeout; pipes/TTY/files) |
| 8 | `lseek` | done |
| 9 | `mmap` | **partial** (`MAP_PRIVATE` anon + file snapshot; file `MAP_SHARED` writeback on `munmap`/exec; no shared-anon / EOF-extend) |
| 10 | `mprotect` | done |
| 11 | `munmap` | done |
| 12 | `brk` | done (per-process break) |
| 13 | `rt_sigaction` | done (handler / `SIG_IGN` / `SIG_DFL`; no full `siginfo`) |
| 14 | `rt_sigprocmask` | done (64-bit mask) |
| 15 | `rt_sigreturn` | done (restorer trampoline) |
| 16 | `ioctl` | done (P11; console + PTY `TCGETS`/`TCGETS2`/winsize/pgrp/`TIOCSCTTY`; master `TIOCGPTN`/`TIOCSPTLCK`) |
| 17 | `pread64` | done (ld.so) |
| 19 | `readv` | done |
| 20 | `writev` | done |
| 21 | `access` | done |
| 22 | `pipe` | done (P7d; cooperative) |
| 23 | `select` | done (P9c; fd_set first 32 fds) |
| 32 | `dup` | done |
| 33 | `dup2` | done |
| 35 | `nanosleep` | done (PIT-based; interruptible by fatal signals) |
| 39 | `getpid` | done (**tgid**) |
| 40 | `sendfile` | partial |
| 56 | `clone` | done (`CLONE_VM` / `FILES` / `THREAD` / settids / TLS / stack) |
| 57 | `fork` | done (private CR3 via `clone_mm`; child Ready) |
| 59 | `execve` | done (ELF64; argv; envp ignored; nested enter) |
| 60 | `exit` | done (one task → zombie; clear_child_tid wake) |
| 61 | `wait4` | done |
| 62 | `kill` | done (process / `pid==0` / `-pgid` group; default terminate + handlers) |
| 63 | `uname` | done (`sysname=munux`; see strings above) |
| 72 | `fcntl` | partial (GET/SET FD/FL, DUPFD) |
| 79 | `getcwd` | done |
| 80 | `chdir` | done |
| 82 | `rename` | done (vops) |
| 83 | `mkdir` | done |
| 84 | `rmdir` | done |
| 86 | `link` | done (vops; hard link; last symlink not followed) |
| 87 | `unlink` | done |
| 88 | `symlink` | done (ext2; fast ≤60 B or one block) |
| 89 | `readlink` | done |
| 90 | `chmod` | done |
| 92 | `chown` | **stub** (always success; single-user) |
| 95 | `umask` | done |
| 96 | `gettimeofday` | done |
| 102 | `getuid` | done |
| 104 | `getgid` | done (0) |
| 105 | `setuid` | **stub** (no-op) |
| 106 | `setgid` | **stub** (no-op) |
| 107 | `geteuid` | done |
| 108 | `getegid` | done (0) |
| 109 | `setpgid` | done (P11a; self/child, same session) |
| 110 | `getppid` | done |
| 111 | `getpgrp` | done (P11a) |
| 112 | `setsid` | done (P11a; new sid=pgid=pid, drop ctty) |
| 115 | `getgroups` | done |
| 121 | `getpgid` | done (P11a) |
| 124 | `getsid` | done (P11a) |
| 157 | `prctl` | done (P9d; name/dumpable/nnp/pdeathsig/seccomp-get/ptracer) |
| 158 | `arch_prctl` | done (`ARCH_SET/GET_FS/GS`; TLS) |
| 162 | `sync` | **stub** (no-op; write-through FS) |
| 175 | `init_module` | done (MNX1 image; params ignored) |
| 176 | `delete_module` | done (`EBUSY` if refcount > 0) |
| 186 | `gettid` | done (unique task id; ≠ tgid for threads) |
| 200 | `tkill` | done (thread-directed signal) |
| 202 | `futex` | done (`WAIT`/`WAKE`/`REQUEUE`/`CMP_REQUEUE` + bitset + relative timeout; `PRIVATE`) |
| 217 | `getdents64` | done |
| 218 | `set_tid_address` | done (stores `clear_child_tid`; wake on exit) |
| 228 | `clock_gettime` | done (REALTIME / MONOTONIC @ 100 Hz) |
| 231 | `exit_group` | done (tear down thread group; one zombie) |
| 234 | `tgkill` | done (tgid + tid directed) |
| 235 | `utimes` | done |
| 257 | `openat` | done (`AT_FDCWD` / abs / dirfd relative) |
| 258 | `mkdirat` | partial |
| 261 | `futimesat` | partial |
| 262 | `newfstatat` | done |
| 263 | `unlinkat` | partial |
| 264 | `renameat` | partial |
| 266 | `symlinkat` | partial (`AT_FDCWD` / abs) |
| 267 | `readlinkat` | partial (`AT_FDCWD` / abs) |
| 268 | `fchmodat` | partial |
| 269 | `faccessat` | partial |
| 280 | `utimensat` | partial |
| 293 | `pipe2` | done (flags ignored) |
| 213 | `epoll_create` | done |
| 232 | `epoll_wait` | done (level-triggered) |
| 233 | `epoll_ctl` | done (ADD/DEL/MOD) |
| 271 | `ppoll` | done (sigmask ignored) |
| 273 | `set_robust_list` | stub (glibc) |
| 291 | `epoll_create1` | done (CLOEXEC ignored) |
| 302 | `prlimit64` | stub (unlimited) |
| 313 | `finit_module` | done (load from fd; MNX1 or ELF ET_REL) |
| 318 | `getrandom` | done (timer-mixed) |
| 322 | `execveat` | done (P9d; `AT_FDCWD` / dirfd relative / `AT_EMPTY_PATH`; `AT_SYMLINK_NOFOLLOW`) |
| 332 | `statx` | done (basic stats; `AT_SYMLINK_NOFOLLOW`) |
| 334 | `rseq` | stub (glibc probe) |
| 435 | `clone3` | done (P10d; `clone_args` flags…tls; stack+size; child inherits GPRs) |

**Notable still ENOSYS** (among others): `pselect6`, `epoll_pwait`,
`vfork`, `dup3`, `statfs`, `getpriority`/`setpriority`, sockets.
Full matrix: **[SYSCALL_COMPARE.md](SYSCALL_COMPARE.md)**.

Unimplemented numbers return **`-ENOSYS` (`-38`)** and log `syscall: ENOSYS n=…`.

### Error returns

- Success: `>= 0` as documented by the call  
- Failure: **`rax = -errno`** (e.g. `-ENOENT` = `-2`, `-ENOSYS` = `-38`)

Common errno values:

| errno | Value | Meaning |
|-------|------:|---------|
| EPERM | 1 | Operation not permitted |
| ENOENT | 2 | No such file |
| ESRCH | 3 | No such process |
| ENOEXEC | 8 | Exec format error |
| EBADF | 9 | Bad file descriptor |
| ECHILD | 10 | No child processes |
| EAGAIN | 11 | Try again |
| ENOMEM | 12 | Out of memory |
| EFAULT | 14 | Bad address |
| EBUSY | 16 | Device or resource busy (e.g. `rmmod` while open) |
| EEXIST | 17 | File exists |
| ENOTDIR | 20 | Not a directory |
| EISDIR | 21 | Is a directory |
| EINVAL | 22 | Invalid argument |
| EMFILE | 24 | Too many open files |
| ENOTTY | 25 | Not a TTY |
| EPIPE | 32 | Broken pipe |
| ERANGE | 34 | Result too large |
| ENAMETOOLONG | 36 | Name too long |
| ENOSYS | 38 | Not implemented |
| ENOTEMPTY | 39 | Directory not empty |
| ELOOP | 40 | Too many symbolic links |
| ETIMEDOUT | 110 | Futex relative wait timed out |

---

## 3. File descriptors

| FD | Name | Backend (today) |
|----|------|-----------------|
| 0 | stdin | keyboard ring (`read`); no `poll` yet (ENOSYS) |
| 1 | stdout | VGA console / file |
| 2 | stderr | VGA console / file |

- Max FDs per table: **32** (see `fd` module).
- **FD tables**: fork **clones** the parent table; `CLONE_FILES` **shares** (refcount).
- `pipe` / `pipe2` / `dup` / `dup2` **are** dispatched (P7d). Freestanding `/bin/sh` does **not** parse `|` — pipelines need a userspace shell that issues `pipe` itself (e.g. BusyBox ash). `dup3` is still ENOSYS.

### `read` on stdin

- May block until data is available.
- Marks the process as TTY foreground (Ctrl-C target preference).
- Console: byte stream from keyboard; **Ctrl-C → SIGINT** (via `tty` + keyboard).
- PTY master input: **n_tty** — `ICANON` (line + erase), `ECHO`, `ISIG` (`^C`/`^\` → fg pgrp).

---

## 4. Process model

| Item | Behavior |
|------|----------|
| Boot | `kinit` = tid/tgid **1**; handoff to userspace `/bin/sh` as a child |
| `getpid` | **tgid** (thread-group id) |
| `gettid` | Unique task id |
| `fork` | Private CR3 (`clone_mm`) + stack copy; FDs cloned; child Ready; parent continues |
| `clone` | Flags include `CLONE_VM` / `CLONE_FILES` / `CLONE_THREAD` / settid / TLS / stack |
| `clone3` | `struct clone_args` (≥64 B); `stack` is the low address; child RSP = stack+size; GPRs inherited (glibc `rdx`=fn / `r8`=arg) |
| `execve` | ELF64 `ET_EXEC`/`ET_DYN`; `PT_INTERP`; `ET_DYN` bias `PIE_BASE`/`INTERP_BASE`; `AT_BASE`/`AT_ENTRY`; enter interp |
| `execveat` | Same image path; `AT_FDCWD` / dirfd relative / `AT_EMPTY_PATH` (`fexecve`) |
| `prctl` | `PR_SET/GET_NAME`, dumpable, `NO_NEW_PRIVS`, `PDEATHSIG`; unknown → `EINVAL` |
| `exit` | One task → zombie; parent woken |
| `exit_group` | Tear down thread group; one zombie for wait |
| `wait4` | Reap zombie; schedules Ready children cooperatively |
| Signals | `kill`/`tkill`/`tgkill`, `rt_sigaction`/`rt_sigprocmask`/`rt_sigreturn`; default terminate + user handlers; `kill(-pgid)` |
| Session | `sid`/`pgid` on PCB; `setsid`/`setpgid`/`getpgrp`/`getpgid`/`getsid`; fork inherits |
| TTY | Console stdio is a tty; **PTY pair** `/dev/ptmx` + `/dev/pts/N`; master input cooked (`ICANON`/`ECHO`/`ISIG`) |
| Futex | `FUTEX_WAIT`/`WAKE`/`REQUEUE`/`CMP_REQUEUE` (+PRIVATE, relative timeout); `clear_child_tid` wake on exit |
| cwd | Per-process (not yet real `CLONE_FS` object share) |
| TLS | Per-task `fs_base` / `gs_base`; `arch_prctl`; `CLONE_SETTLS`; do not reload FS/GS selectors on enter (would clear base MSRs) |
| Scheduling | Timer user→user preempt; nest depth ≥ 2 stays cooperative |

**Userspace shell:** freestanding `/bin/sh` (builtins + fork/exec); ignores SIGINT/SIGQUIT. BusyBox applets used as probes toward a full Linux userspace / desktop.

**Kernel debug shell:** after userspace `exit` (`munux>`): `preempttest`, `run sh`, `ps`, …

---

## 5. Memory layout (userspace, typical)

| Region | Notes |
|--------|--------|
| Low ET_EXEC (e.g. `0x400000`) | Static ELF load |
| brk heap | Grows from image end |
| mmap arena | Process `mmap_bump` (anonymous) |
| Classic stack | Near `USER_STACK_TOP` (`0x7FFF_F000`) |
| Signal restorer | `0x7ffd0000` — kernel trampoline (`rt_sigreturn`) |

See **[MM.md](MM.md)** for kernel windows and isolation rules.

---

## 6. Filesystem

- Root: **ext2** on IDE primary master.
- Virtual **`/proc`**: kernel-generated (`meminfo`, `mounts`, `modules`, pid nodes, …).
- Virtual **`/dev`**: `null`, `zero`, `hda`, **`ptmx`**, **`pts/`**; `echo` only after `insmod echo.mnx`.
- Virtual **`/ram`**: ramfs.
- Live `/proc/mounts` (qemu-connect 2026-08-07): `/dev/hda / ext2`, `ramfs /ram`, `proc /proc`, `devtmpfs /dev`.
- Writes: create/unlink/mkdir/rmdir/chmod/rename/link as wired in syscalls → vops → ext2.
- Symlinks: `symlink`/`readlink`/`lstat` vs `stat` follow; max 8 hops (`ELOOP`).
- **Not yet:** `mount`/`umount` syscalls; full dentry cache; `msync`; shared-anon mmap.

---

## 7. What is still required for fuller Linux binaries

Foundation for threads is in; polish and next architecture:

1. ~~Per-process page tables + fork without snapshots~~ ✅  
2. ~~Scheduler + `clone` + futex~~ ✅ (practical slices)  
3. ~~Signals (`rt_sigreturn`, delivery, `tgkill`, Ctrl-C)~~ ✅ (practical slices)  
4. Full signal frames (`siginfo`/`ucontext`), absolute/PI futex, musl pthread soak  
5. ~~`pipe`/`dup`, `rename`/`link`, VFS + modules~~ ✅ P7–P8c  
6. ~~`readlink`/`symlink`/`statx`~~ ✅ P9a; ~~file mmap~~ ✅ P9b; ~~poll/select/epoll~~ ✅ P9c; ~~`execveat`/`prctl`~~ ✅ P9d; ~~file-map ELF + `MAP_SHARED`~~ ✅ P9e  
7. ~~session/pgrp + console termios~~ ✅ P11a; ~~PTY pair~~ ✅ P11b; ~~n_tty cook~~ ✅ P11c; `SIGTTOU`/`SIGTTIN` still open

Using **wrong syscall numbers** would make Linux binaries impossible — munux keeps Linux numbers from v0.2 forward.

---

## History

| Ver | Notes |
|-----|--------|
| 0.1 | Custom numbers (EXIT=0, WRITE=1, …) |
| **0.2** | **Linux x86_64 numbers** + `-errno` |
| 0.2+U3–U8 | open/read files, getdents, PCB, fork/exec, freestanding sh, boot handoff |
| 0.2+FD | Per-process FD tables |
| 0.2+TLS | `arch_prctl`, nest-safe enter_user TLS |
| 0.2+BusyBox | Large static ELF, brk/mmap, FS syscalls, ash bring-up |
| **0.3** | Private mm, preempt, `clone`/tid, signals, futex (2026-08) |
| **0.3.1** | Doc sync with P7d/P8 dispatch: pipe/dup/rename/link/modules (2026-08-07) |
| **0.3.2** | P9a: `symlink`/`readlink`/`statx`; **81** dispatched (2026-08-07) |
| **0.3.3** | P9c: poll/select/epoll; **88** dispatched |
| **0.3.4** | P9d: `execveat`/`prctl`; **90** dispatched (2026-08-09) |
| **0.3.5** | P9e: inode `PT_LOAD` stream + file `MAP_SHARED` writeback |
| **0.3.6** | P10a: `PT_INTERP` + auxv `AT_BASE`; smoke `dynlinktest` |
| **0.3.7** | P10b: `ET_DYN` load bias; smoke `dynlinkpie` |
| **0.3.8** | P10c: glibc `ld.so`+`libc` `hello_dyn`; pread64/getrandom/prlimit64/rseq/robust_list |
| **0.3.9** | P10d: `clone3` + `CLONE_SETTLS` + GPR inherit; smokes `tlsclone` / glibc `clonec` |
| **0.3.10** | P11a: `setsid`/`setpgid`/`getpgrp`/`getpgid`/`getsid`; console termios + `TIOCSCTTY`; smoke `jobtest` |
| **0.3.11** | P11b: `/dev/ptmx` + `/dev/pts/N`; smoke `ptytest` |
| **0.3.12** | P11c: PTY n_tty (`ICANON`/`ECHO`/`ISIG`); smoke `n_ttytest` |
