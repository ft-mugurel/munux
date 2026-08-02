# munux ABI (working specification)

This document freezes conventions for userspace and the kernel.  
**Change only with a deliberate version bump.**

| Field | Value |
|-------|--------|
| **Status** | **v0.3** — Linux x86_64 syscall numbers; expanded surface |
| **Arch** | **x86_64** only on `main` |
| **Goal** | Static Linux/musl binaries use the **same numbers and register ABI** as Linux; missing calls return **`-ENOSYS`** |

Product direction (threads, modules, per-process mm): **[ROADMAP.md](ROADMAP.md)**.  
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
Source of truth: `src/syscalls/mod.rs` dispatch `match`.  
~**65** numbers are dispatched (quality varies: full / partial / stub).

| # | Linux name | munux status |
|--:|------------|--------------|
| 0 | `read` | done (stdin, files) |
| 1 | `write` | done (console, files) |
| 2 | `open` | done (`O_CREAT` / `O_TRUNC` / R/W) |
| 3 | `close` | done |
| 4 | `stat` | done |
| 5 | `fstat` | done |
| 6 | `lstat` | done (no real symlinks yet → like stat) |
| 8 | `lseek` | done |
| 9 | `mmap` | **partial** (anonymous `MAP_PRIVATE`; offset forced 0 in entry) |
| 10 | `mprotect` | done |
| 11 | `munmap` | done |
| 12 | `brk` | done (per-process break) |
| 13 | `rt_sigaction` | done (handler / `SIG_IGN` / `SIG_DFL`; no full `siginfo`) |
| 14 | `rt_sigprocmask` | done (64-bit mask) |
| 15 | `rt_sigreturn` | done (restorer trampoline) |
| 16 | `ioctl` | **partial** (TTY probes) |
| 19 | `readv` | done |
| 20 | `writev` | done |
| 21 | `access` | done |
| 35 | `nanosleep` | done (PIT-based; interruptible by fatal signals) |
| 39 | `getpid` | done (**tgid**) |
| 40 | `sendfile` | partial |
| 56 | `clone` | done (`CLONE_VM` / `FILES` / `THREAD` / settids / TLS / stack) |
| 57 | `fork` | done (private CR3 via `clone_mm`; child Ready) |
| 59 | `execve` | done (ELF64; argv; envp ignored; nested enter) |
| 60 | `exit` | done (one task → zombie; clear_child_tid wake) |
| 61 | `wait4` | done |
| 62 | `kill` | done (process-directed; default terminate + handlers) |
| 63 | `uname` | done (`sysname=munux`) |
| 72 | `fcntl` | partial (GET/SET FD/FL, DUPFD) |
| 79 | `getcwd` | done |
| 80 | `chdir` | done |
| 83 | `mkdir` | done |
| 84 | `rmdir` | done |
| 87 | `unlink` | done |
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
| 110 | `getppid` | done |
| 115 | `getgroups` | done |
| 158 | `arch_prctl` | done (`ARCH_SET/GET_FS/GS`; TLS) |
| 162 | `sync` | **stub** (no-op; write-through FS) |
| 186 | `gettid` | done (unique task id; ≠ tgid for threads) |
| 200 | `tkill` | done (thread-directed signal) |
| 202 | `futex` | done (`WAIT`/`WAKE`/`REQUEUE`/`CMP_REQUEUE` + bitset + relative timeout; `PRIVATE`) |
| 217 | `getdents64` | done |
| 218 | `set_tid_address` | done (stores `clear_child_tid`; wake on exit) |
| 228 | `clock_gettime` | done (REALTIME / MONOTONIC @ 100 Hz) |
| 231 | `exit_group` | done (tear down thread group; one zombie) |
| 234 | `tgkill` | done (tgid + tid directed) |
| 235 | `utimes` | done |
| 257 | `openat` | partial (`AT_FDCWD` / absolute) |
| 258 | `mkdirat` | partial |
| 261 | `futimesat` | partial |
| 262 | `newfstatat` | done |
| 263 | `unlinkat` | partial |
| 268 | `fchmodat` | partial |
| 269 | `faccessat` | partial |
| 280 | `utimensat` | partial |

**Notable still ENOSYS** (among others): `pipe`/`pipe2`, `dup`/`dup2`, `poll`/`ppoll`,
`rename`/`renameat`, `link`, `readlink`/`symlink`, `vfork`, `statfs`, `getpriority`/`setpriority`,
`setpgid`/`getpgrp`/`setsid`. Full matrix: **[SYSCALL_COMPARE.md](SYSCALL_COMPARE.md)**.

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
| EEXIST | 17 | File exists |
| ENOTDIR | 20 | Not a directory |
| EISDIR | 21 | Is a directory |
| EINVAL | 22 | Invalid argument |
| ENOTTY | 25 | Not a TTY |
| ENAMETOOLONG | 36 | Name too long |
| ENOSYS | 38 | Not implemented |
| ENOTEMPTY | 39 | Directory not empty |
| ERANGE | 34 | Result too large |
| EMFILE | 24 | Too many open files |

---

## 3. File descriptors

| FD | Name | Backend (today) |
|----|------|-----------------|
| 0 | stdin | keyboard ring (`read`); ash may `poll` |
| 1 | stdout | VGA console / file |
| 2 | stderr | VGA console / file |

- Max FDs per table: **32** (see `fd` module).
- **FD tables**: fork **clones** the parent table; `CLONE_FILES` **shares** (refcount).
- `pipe` / `dup` / `dup2` are **not** dispatched yet (ENOSYS).

### `read` on stdin

- May block until data is available.
- Marks the process as TTY foreground (Ctrl-C target preference).
- Byte stream; no full line discipline. **Ctrl-C → SIGINT** (via `tty` + keyboard).

---

## 4. Process model

| Item | Behavior |
|------|----------|
| Boot | `kinit` = tid/tgid **1**; handoff to userspace `/bin/sh` as a child |
| `getpid` | **tgid** (thread-group id) |
| `gettid` | Unique task id |
| `fork` | Private CR3 (`clone_mm`) + stack copy; FDs cloned; child Ready; parent continues |
| `clone` | Flags include `CLONE_VM` / `CLONE_FILES` / `CLONE_THREAD` / settid / TLS / stack |
| `execve` | Load ELF64 into current task; argv copied; envp ignored |
| `exit` | One task → zombie; parent woken |
| `exit_group` | Tear down thread group; one zombie for wait |
| `wait4` | Reap zombie; schedules Ready children cooperatively |
| Signals | `kill`/`tkill`/`tgkill`, `rt_sigaction`/`rt_sigprocmask`/`rt_sigreturn`; default terminate + user handlers |
| Futex | `FUTEX_WAIT`/`WAKE` (+PRIVATE); `clear_child_tid` wake on exit |
| cwd | Per-process (not yet real `CLONE_FS` object share) |
| TLS | Per-task `fs_base` / `gs_base`; `arch_prctl` |
| Scheduling | Timer user→user preempt; nest depth ≥ 2 stays cooperative |

**Userspace shell:** freestanding `/bin/sh` (builtins + fork/exec); ignores SIGINT/SIGQUIT. BusyBox applets used as probes.

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
- Virtual **`/proc`**: kernel-generated (`meminfo`, `mounts`, pid nodes, …).
- Writes: create/unlink/mkdir/rmdir/chmod as wired in syscalls → `fs/ext2_write.rs`.
- **Not yet:** `rename`/`link`/`symlink`/`readlink` as dispatched syscalls; full VFS ops tables (roadmap P7).

---

## 7. What is still required for fuller Linux binaries

Foundation for threads is in; polish and next architecture:

1. ~~Per-process page tables + fork without snapshots~~ ✅  
2. ~~Scheduler + `clone` + futex~~ ✅ (practical slices)  
3. ~~Signals (`rt_sigreturn`, delivery, `tgkill`, Ctrl-C)~~ ✅ (practical slices)  
4. Full signal frames (`siginfo`/`ucontext`), futex timeout, musl pthread soak  
5. **`pipe`/`dup`**, **`rename`/`link`/`readlink`**, file-backed `mmap`, dynamic linker path  
6. **VFS** + **kernel modules** (roadmap P7–P8)  
7. Broader syscall surface (network optional)

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
| **0.3** | Private mm, preempt, `clone`/tid, signals, futex (2026-08); ~65 dispatched |
