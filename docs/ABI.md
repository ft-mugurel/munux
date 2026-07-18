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
| 0 | `read` | **done** (stdin console) |
| 1 | `write` | **done** (stdout/stderr console) |
| 2 | `open` | planned U3 (`-ENOSYS`) |
| 3 | `close` | **done** |
| 39 | `getpid` | **done** (returns 1 for now) |
| 57 | `fork` | planned U6 |
| 59 | `execve` | planned U6 |
| 60 | `exit` | **done** |
| 61 | `wait4` | planned U5 |
| 79 | `getcwd` | planned U3 |
| 80 | `chdir` | planned U3 |
| 231 | `exit_group` | **done** (same as `exit` for now) |
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
- v0.2: one global FD table; per-process in U5.

### `read` on stdin

- Blocks (`sti; hlt`) until ≥1 byte is available.
- Returns `min(available, len)` (byte stream; no kernel line discipline).

---

## 4. Process model (sketch)

| Item | Today |
|------|--------|
| `getpid` | returns `1` |
| `exit` / `exit_group` | return to kernel launcher (`user` / `run`) |
| Future init | `execve("/bin/sh")` as pid 1 |

---

## 5. What is still required for real Linux binaries

Numbers alone are **not** enough. Also needed over time:

- Many more syscalls (`brk`/`mmap`, `arch_prctl`, `uname`, `set_tid_address`, …)
- Full pointer validation and signal/`rt_sigreturn` paths
- ELF aux vector completeness for dynamic linker (later)
- Correct `errno` coverage for each call

But using **wrong numbers guarantees** Linux binaries will never work — so munux uses Linux numbers from this version forward.

---

## History

| Ver | Notes |
|-----|--------|
| 0.1 | Custom numbers (EXIT=0, WRITE=1, …) |
| **0.2** | **Linux x86_64 numbers** + `-errno` returns |
