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
| 1 | `write` | **done** (stdout/stderr console) |
| 2 | `open` | **done** (files + directories; dirs for getdents) |
| 3 | `close` | **done** |
| 39 | `getpid` | **done** (real PCB pid) |
| 57 | `fork` | planned U6 |
| 59 | `execve` | planned U6 |
| 60 | `exit` | **done** (zombie + return to parent) |
| 61 | `wait4` | **done** (reap zombie; non-blocking) |
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
- Still **one global FD table** (shared). True per-process FD tables come with full `fork` (U6). Stdio works for cooperative single-user tasks.

### `read` on stdin

- Blocks (`sti; hlt`) until ≥1 byte is available.
- Returns `min(available, len)` (byte stream; no kernel line discipline).

---

## 4. Process model (U5)

| Item | Behavior |
|------|----------|
| Boot | `init` = pid **1** (kernel shell) |
| `run` / `user` | spawn child PCB, switch current → child, enter ring 3 |
| `getpid` | current process pid |
| `getppid` | parent pid (`0` if none) |
| `exit` / `exit_group` | mark **zombie**, switch current → parent, return to launcher |
| `wait4` | reap a zombie child; Linux status `((code & 0xff) << 8)`; **non-blocking** (no scheduler sleep yet): returns `0` if children exist but none zombie, `-ECHILD` if no children |
| cwd | **per-process** (`chdir` / `getcwd` use current PCB) |

Future init: `execve("/bin/sh")` as pid 1 (U7–U8).

---

## 5. What is still required for real Linux binaries

Numbers alone are **not** enough. Also needed over time:

- Many more syscalls (`brk`/`mmap`, `arch_prctl`, `uname`, `set_tid_address`, …)
- Full pointer validation and signal/`rt_sigreturn` paths
- ELF aux vector completeness for dynamic linker (later)
- Correct `errno` coverage for each call
- `fork` + `execve` and per-process FD tables

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
