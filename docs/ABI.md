# munux ABI (working specification)

This document freezes conventions for userspace and the kernel.  
**Change only with a deliberate version bump** — do not silently renumber syscalls.

Status: **v0.1** (pre-Linux compatibility). Target arch: **x86_64**.

---

## 1. Calling convention (`syscall`)

| Item | Value |
|------|--------|
| Instruction | `syscall` / return `sysret` (64-bit) |
| Number | `rax` |
| Args | `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` (Linux x86_64 **register** layout) |
| Return | `rax` |
| Clobbered by CPU | `rcx` (saved RIP), `r11` (saved RFLAGS) |

Kernel entry: `IA32_LSTAR` → `syscall_entry`.  
`IA32_STAR` selects kernel CS/SS and user CS/SS for `sysret`.

---

## 2. Syscall numbers (v0.1)

These are **munux-native**, not Linux numbers yet.  
A future epic may renumber to Linux x86_64; that will be an explicit ABI break.

| # | Name | Args | Return |
|---|------|------|--------|
| 0 | `EXIT` | `rdi` = status | does not return to caller |
| 1 | `WRITE` | `rdi`=fd, `rsi`=buf, `rdx`=len | bytes written, or error |
| 2 | `READ` | *(planned U2)* | |
| 3 | `OPEN` | *(planned U3)* | |
| 4 | `CLOSE` | `rdi`=fd | 0 or error |
| 5 | `GETPID` | — | pid |

### Error returns (v0.1)

- Success: non-negative value as documented.
- Failure: **`rax == u64::MAX`** (`!0`) for now.
- **Planned:** switch to Linux-style **negative errno** in a later PR.

User code must treat `!0` as error until errno is introduced.

---

## 3. File descriptors

### Standard FDs (installed for every process at start)

| FD | Name | v0.1 backend |
|----|------|----------------|
| 0 | stdin | console / keyboard (read in U2) |
| 1 | stdout | VGA console write |
| 2 | stderr | VGA console write (same device) |

### Rules

- FDs are small non-negative integers indexing an **FD table** (v0.1: one global table until multi-process U5).
- `WRITE` only succeeds on FDs that support write (stdout/stderr today).
- `CLOSE` frees a slot; closing 0/1/2 is allowed but not recommended.
- Max open FDs per table: **32** (`FD_MAX`).

### Path / open (future)

- Paths are Unix-style, `/` absolute, otherwise relative to process **cwd** (inode on ext2).
- `OPEN` will return a new FD or error.

---

## 4. Process model (v0.1 sketch)

| Item | v0.1 |
|------|------|
| Processes | Single logical “current” userspace context; PCB table in U5 |
| pid of userspace demo | `1` from `GETPID` today |
| `EXIT` from ring 3 | Returns to kernel helper that started the program (shell `user`/`run`) |
| Future init | Kernel mounts FS, then `execve("/bin/sh")` as pid 1 |
| If `/bin/sh` missing | Fall back to kernel shell (`munux>`) |

---

## 5. Memory / pointers from userspace

- Syscall buffers must point at **user-accessible** memory (validated by kernel).
- Max single `WRITE`/`READ` length: **4096** bytes (v0.1).
- Kernel must not trust user pointers without checks.

---

## 6. User entry

- Ring 3 entry via `iretq` (`enter_user_mode`).
- User CS = `0x23`, user SS/DS = `0x1B` (GDT layout for STAR).
- Stack and code pages mapped with **USER** bit.

---

## 7. Roadmap

U1 FD table + WRITE via FD (this milestone) → U2 READ → U3 OPEN → … → `/bin/sh` as init.

| Ver | Notes |
|-----|--------|
| 0.1 | Initial freeze: syscall regs, numbers 0/1/4/5, FD 0/1/2, error=`!0` |
