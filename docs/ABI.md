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
~**80** numbers are dispatched (quality varies: full / partial / stub).

| # | Linux name | munux status |
|--:|------------|--------------|
| 0 | `read` | done (stdin, files, pipes) |
| 1 | `write` | done (console, files, pipes) |
| 2 | `open` | done (`O_CREAT` / `O_TRUNC` / R/W) |
| 3 | `close` | done |
| 4 | `stat` | done |
| 5 | `fstat` | done |
| 6 | `lstat` | done (no real symlinks yet → like stat) |
| 7 | `poll` | done (ash / TTY) |
| 8 | `lseek` | done |
| 9 | `mmap` | **partial** (anonymous `MAP_PRIVATE`; `MAP_FIXED` / `PROT_NONE` guards; offset forced 0 in entry) |
| 10 | `mprotect` | done |
| 11 | `munmap` | done |
| 12 | `brk` | done (per-process break) |
| 13 | `rt_sigaction` | **stub** (stores intent; no real delivery) |
| 14 | `rt_sigprocmask` | **stub** (returns success) |
| 16 | `ioctl` | **partial** (TTY probes, winsize, DSR inject) |
| 19 | `readv` | done |
| 20 | `writev` | done |
| 21 | `access` | done |
| 22 | `pipe` | done |
| 32 | `dup` | done |
| 33 | `dup2` | done |
| 35 | `nanosleep` | done (PIT-based) |
| 39 | `getpid` | done |
| 40 | `sendfile` | partial |
| 57 | `fork` | done (cooperative; **shared AS** + image restore) |
| 58 | `vfork` | alias of `fork` |
| 59 | `execve` | done (ELF64; argv; envp ignored; nested enter) |
| 60 | `exit` | done |
| 61 | `wait4` | done |
| 62 | `kill` | **partial** |
| 63 | `uname` | done (`sysname=munux`) |
| 72 | `fcntl` | partial (GET/SET FD/FL, DUPFD) |
| 79 | `getcwd` | done |
| 80 | `chdir` | done |
| 82 | `rename` | done (ext2; BusyBox `mv`) |
| 83 | `mkdir` | done |
| 84 | `rmdir` | done |
| 86 | `link` | done (hard link) |
| 87 | `unlink` | done |
| 89 | `readlink` | **const only — still ENOSYS if called** |
| 90 | `chmod` | done |
| 92 | `chown` | **stub** (always success; single-user) |
| 95 | `umask` | done |
| 96 | `gettimeofday` | done |
| 99 | `sysinfo` | done (`free`) |
| 102 | `getuid` | done |
| 104 | `getgid` | done |
| 105 | `setuid` | **stub** |
| 106 | `setgid` | **stub** |
| 107 | `geteuid` | done |
| 108 | `getegid` | done |
| 109 | `setpgid` | **stub** |
| 110 | `getppid` | done |
| 111 | `getpgrp` | done |
| 112 | `setsid` | partial |
| 115 | `getgroups` | done |
| 121 | `getpgid` | done |
| 137 | `statfs` | done (`df`) |
| 138 | `fstatfs` | done |
| 140 | `getpriority` | done (Linux kernel nice encoding) |
| 141 | `setpriority` | done |
| 158 | `arch_prctl` | done (`ARCH_SET/GET_FS/GS`; TLS) |
| 162 | `sync` | **stub** (no-op; write-through FS) |
| 186 | `gettid` | = `getpid` (no threads yet) |
| 217 | `getdents64` | done |
| 218 | `set_tid_address` | done (return tid; clear_child_tid wake **not** yet) |
| 228 | `clock_gettime` | done (REALTIME / MONOTONIC @ 100 Hz) |
| 231 | `exit_group` | same as `exit` for now |
| 235 | `utimes` | done |
| 257 | `openat` | partial (`AT_FDCWD` / absolute) |
| 258 | `mkdirat` | partial |
| 261 | `futimesat` | partial |
| 262 | `newfstatat` | done |
| 263 | `unlinkat` | partial |
| 264 | `renameat` | partial |
| 268 | `fchmodat` | partial |
| 269 | `faccessat` | partial |
| 271 | `ppoll` | partial |
| 280 | `utimensat` | partial |
| 293 | `pipe2` | done |

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
- **Per-process FD tables**: fork **clones** the parent table (independent offsets).
- Pipes: `pipe` / `pipe2` + `dup2` for redirections / ash.

### `read` on stdin

- May block until data is available (policy depends on path).
- Byte stream; no full line discipline.

---

## 4. Process model

| Item | Behavior |
|------|----------|
| Boot | `kinit` = pid **1**; handoff to userspace `/bin/sh` as a child |
| `getpid` / `getppid` | PCB fields |
| `fork` / `vfork` | New PCB; child **private stack**; **shared page tables** today; FDs cloned; child `rax=0`. Cooperative: often runs to completion **inside** the parent’s `fork` |
| `execve` | Load ELF64 into current task; argv copied; envp ignored. On shared AS, parent image is **snapshotted/restored** around nested exec |
| `exit` / `exit_group` | Zombie → switch to parent → `return_from_user` (nest pop) |
| `wait4` | Reap zombie; `WNOHANG` supported |
| cwd | Per-process |
| TLS | Per-process `fs_base` / `gs_base`; enter_user reinstalls MSRs with **null FS/GS selectors** (Linux long-mode convention) |
| nice | Stored; `getpriority`/`setpriority` Linux encoding; scheduler does not weight yet |

**Not yet:** preemptive multi-tasking, per-process CR3, `clone` threads, futex, real signal delivery.

**Userspace shell:** freestanding `/bin/sh` (builtins + fork/exec). BusyBox **ash** also runs for many interactive cases.

**Kernel debug shell:** after userspace `exit`, prompt returns to kernel-side commands (`run sh`, `ps`, …).

---

## 5. Memory layout (userspace, typical)

| Region | Notes |
|--------|--------|
| Low ET_EXEC (e.g. `0x400000`) | Static ELF load; often identity-backed today |
| brk heap | Grows from image end |
| mmap arena | `0x5000_0000` … `0x6000_0000` (anonymous) |
| Fork child stacks | `0x6F00_0000` + slot stride |
| Classic stack | Near `USER_STACK_TOP` (`0x7FFF_F000`) for initial image |

Kernel high windows used for temporary snapshots (implementation detail; will shrink once private mm lands).

---

## 6. Filesystem

- Root: **ext2** on IDE primary master.
- Virtual **`/proc`**: kernel-generated (`meminfo`, `mounts`, pid nodes, …) — not real proc inodes on disk.
- Writes: create/unlink/mkdir/rmdir/link/**rename**/chmod/truncate paths as implemented in `fs/ext2_write.rs`.

---

## 7. What is still required for fuller Linux binaries

Architecture (see roadmap), then ABI polish:

1. **Per-process page tables** + real fork/exec without snapshots  
2. **Scheduler** + **`clone`** + **futex** (threads)  
3. Real **signals** (`rt_sigreturn`, delivery, `tgkill`)  
4. **`readlink` / `symlink`**, file-backed `mmap`, dynamic linker path  
5. **VFS** + **kernel modules**  
6. Broader syscall surface (network optional)

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
| 0.2+BusyBox | Large static ELF, brk/mmap, many FS syscalls, ash bring-up |
| **0.3** | Documented ~80-call surface; `rename`/`mv`; procfs; roadmap for threads/modules; accurate shared-AS caveats |
