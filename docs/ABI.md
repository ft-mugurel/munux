# munux ABI (working specification)

This document freezes conventions for userspace and the kernel.  
**Change only with a deliberate version bump.**

Status: **v0.2** — **Linux x86_64 syscall numbers** for implemented calls.  
Target arch: **x86_64**.

Goal: a static Linux binary (musl) should use the **same numbers and register ABI** as on Linux; missing syscalls return `-ENOSYS` until implemented.

---

## 1. Calling convention (`syscall`)

Same as Linux x86_64:

| Item | Value |
|------|--------|
| Instruction | `syscall` / return via `sysret` |
| Number | `rax` |
| Args | `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` |
| Return | `rax` (or `-errno` on failure) |
| Clobbered | `rcx` (RIP), `r11` (RFLAGS) |

---

## 2. Syscall numbers (Linux x86_64)

Reference: Linux `arch/x86/entry/syscalls/syscall_64.tbl`.

| # | Linux name | munux status |
|---|------------|--------------|
| 0 | `read` | **done** (stdin or open file FD) |
| 1 | `write` | **done** (console + ext2 file FDs) |
| 2 | `open` | **done** (files + dirs; `O_CREAT`/`O_TRUNC`/`O_WRONLY`/`O_RDWR`) |
| 3 | `close` | **done** |
| 16 | `ioctl` | **done** (stub: returns `-ENOTTY`; enough for musl TIOCGWINSZ probe) |
| 20 | `writev` | **done** (musl stdio / `printf`) |
| 39 | `getpid` | **done** (real PCB pid) |
| 57 | `fork` | **done** (PCB + Ready child; shared AS) |
| 59 | `execve` | **done** (load ELF; argv up to 3 strings; envp ignored) |
| 60 | `exit` | **done** (zombie + return to parent) |
| 61 | `wait4` | **done** (reap; schedules Ready children) |
| 63 | `uname` | **done** (struct utsname; sysname=munux, machine=x86_64) |
| 9 | `mmap` | **done** (anonymous `MAP_PRIVATE`; `MAP_FIXED` + `PROT_NONE` for musl guards) |
| 10 | `mprotect` | **done** (update PTE flags / `PROT_NONE` unmap) |
| 11 | `munmap` | **done** (tracked region or best-effort page unmap) |
| 12 | `brk` | **done** (program break / heap grow; per-process; Linux return = break addr) |
| 158 | `arch_prctl` | **done** (`ARCH_SET/GET_FS`, `ARCH_SET/GET_GS`; per-process + CPU MSRs) |
| 218 | `set_tid_address` | **done** (return pid; clear_child_tid on exit not yet) |
| 79 | `getcwd` | **done** (per-process cwd) |
| 80 | `chdir` | **done** (per-process cwd) |
| 110 | `getppid` | **done** |
| 231 | `exit_group` | **done** (same as `exit` for now) |
| 217 | `getdents64` | **done** (directory listing) |
| 257 | `openat` | planned (modern libc) |

Unimplemented numbers return **`-ENOSYS`** (`-38`).

### Error returns (Linux-style)

- Success: `>= 0` as documented by the syscall.
- Failure: **`rax = -errno`** as a 64-bit two's complement value  
  (e.g. `-EBADF` = `-9`, `-EFAULT` = `-14`, `-ENOSYS` = `-38`).

User code should check `(long)ret < 0`.

Common errno values we use:

| errno | Value | Meaning |
|-------|------:|---------|
| EPERM | 1 | Operation not permitted |
| ENOENT | 2 | No such file |
| EBADF | 9 | Bad file descriptor |
| ECHILD | 10 | No child processes |
| EFAULT | 14 | Bad address |
| EINVAL | 22 | Invalid argument |
| ENOSYS | 38 | Not implemented |

---

## 3. File descriptors

| FD | Name | Backend (today) |
|----|------|-----------------|
| 0 | stdin | keyboard ring buffer (`read`) |
| 1 | stdout | VGA console (`write`) |
| 2 | stderr | VGA console (`write`) |

- Max FDs per table: **32**.
- **Per-process FD tables**: each PCB slot has its own table. `fork` / user-task spawn **clone** the parent's open FDs (independent offsets afterward). Closing an FD in a child does not affect the parent.

### `read` on stdin

- Blocks (`sti; hlt`) until ≥1 byte is available.
- Returns `min(available, len)` (byte stream; no kernel line discipline).

---

## 4. Process model (U5–U6)

| Item | Behavior |
|------|----------|
| Boot | `kinit` = pid **1** (kernel idle). U8 hands off to userspace `/bin/sh` as a child |
| `run` / `user` | spawn child PCB, switch current → child, enter ring 3 |
| `getpid` | current process pid |
| `getppid` | parent pid (`0` if none) |
| `fork` | new PCB; child gets **private stack copy**, **cloned FD table**, and `rax=0`; parent stays current. Cooperative: child runs to completion **inside** `fork` before parent resumes |
| `execve` | load ELF into current process (path from FS or embedded); argv/envp ignored; on success never returns to old image. Kernel snapshots parent text/data so shared-AS `execve` does not destroy the waiting parent |
| `exit` / `exit_group` | mark **zombie**, switch current → parent, `return_from_user` (nested enter stack) |
| `wait4` | reap zombie; can also run leftover Ready children; `WNOHANG` skips schedule; `-ECHILD` if no children |
| cwd | **per-process** (`chdir` / `getcwd` use current PCB) |

Cooperative model (no preemptive multi-process scheduler, no private page tables yet). Nested kernel stacks for wait/exec.

**U7:** freestanding `/bin/sh` in userspace. Builtins: `help`, `exit`, `cd`, `pwd`, `clear`. Other words → `fork` + `execve("/bin/<cmd>")` + `wait4` (argv passed).

**U8:** after boot, kernel loads `/bin/sh` (ext2 or embedded) and enters it as the interactive userspace init. Kernel pid 1 remains `kinit` (parent). When sh `exit`s, control returns to the **kernel debug shell** (`munux>`). Re-enter with `run sh` / `run init`.

---

## 5. What is still required for real Linux binaries

Numbers alone are **not** enough. Also needed over time:

- Many more syscalls (`mprotect`, file-backed `mmap`, `set_tid_address`, …)
- Full pointer validation and signal/`rt_sigreturn` paths
- ELF aux vector completeness for dynamic linker (later)
- Correct `errno` coverage for each call
- Real page-table isolation / COW on fork

But using **wrong numbers guarantees** Linux binaries will never work — so munux uses Linux numbers from this version forward.

---

## History

| Ver | Notes |
|-----|--------|
| 0.1 | Custom numbers (EXIT=0, WRITE=1, …) |
| **0.2** | **Linux x86_64 numbers** + `-errno` returns |
| 0.2+U3 | `open` / file `read` / `chdir` / `getcwd` |
| 0.2+U4 | `getdents64` + open directory FDs |
| 0.2+U5 | PCB, real pid/ppid, zombie exit, wait4, per-process cwd |
| 0.2+U6 | `fork` + `execve`; wait schedules Ready children; nested enter |
| 0.2+U7 | Freestanding `/bin/sh` (prompt, builtins, fork/exec/wait) |
| 0.2+U8 | Boot handoff to `/bin/sh`; kernel shell is debug fallback |
| 0.2+FD | Per-process FD tables (clone on fork/spawn) |
| 0.2+uname | `uname` + ENOSYS logging for musl bring-up |
| 0.2+arch_prctl | FS/GS base for TLS (`arch_prctl`) |
