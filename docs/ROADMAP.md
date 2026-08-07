# munux roadmap — Linux-compatible kernel in Rust

**Last updated:** 2026-08-07 (P8 **closed**; IDE stays built-in — not a P8 gap. Next is Phase 9).

**Goal:** a **Linux x86_64 ABI–compatible** kernel written in Rust, not “run every BusyBox applet.”

BusyBox / musl binaries are **compatibility probes** (does `fork`/`clone`/`mmap`/ELF load match Linux?). They are not the product definition.

**Related docs:** [README](../README.md) · [ABI](ABI.md) · [MM](MM.md) · [SYSCALL_COMPARE](SYSCALL_COMPARE.md) · [SMOKE_PREEMPT](SMOKE_PREEMPT.md) · [SMOKE_CLONE](SMOKE_CLONE.md) · [SMOKE_SIGNAL](SMOKE_SIGNAL.md) · [SMOKE_FUTEX](SMOKE_FUTEX.md) · [SMOKE_VFS](SMOKE_VFS.md) · [SMOKE_MODULE](SMOKE_MODULE.md) · [BusyBox suite](BUSYBOX_SUITE_REPORT.md)

**North stars:**

1. **Thread support** (Linux `clone` / TID model / futex) — **foundation in place**
2. **Kernel modules** (loadable objects, symbol export, init/exit) — **P8a–8c done** (MNX1 + ET_REL `.ko`)

---

## 0. Where we are today (honest baseline)

| Area | Current state | Blocker for modules? |
|------|---------------|----------------------|
| Arch | x86_64 long mode, `syscall`/`sysret`, GDT/TSS/IDT | OK |
| Syscall ABI | Linux numbers; growing surface | Partial — OK to grow |
| Memory | **Per-process CR3** + `clone_mm`; identity kernel window | OK for threads; high-half later |
| Processes | PCB tid/tgid; fork private mm; wait/exit_group | OK |
| Scheduling | Timer user→user preempt (`TrapFrame`); nest policy depth ≤ 1 | OK for user threads |
| Threads | **`clone`**, shared mm/files, gettid/tgid | OK (no full musl pthread suite yet) |
| Signals | kill/tgkill, masks, handlers, rt_sigreturn, Ctrl-C | OK for practical use |
| Futex | WAIT/WAKE/REQUEUE + timeout + clear_child_tid | OK basic join |
| FS | **VFS P7 practical**: fops, mounts, proc, mutations, pipes | OK for modules |
| Drivers | chrdev + blkdev; **IDE `hda` is built-in** (root disk) | OK — must stay built-in |
| Modules | **P8a–8c done**: MNX1 + ELF ET_REL `.ko`, `/dev/echo` | Not blocked |

**Implication:** P7 + **P8 complete** (hello/echo as `.mnx` and `.ko`).  
**Next epic: Phase 9** (broader Linux surface). Do **not** treat “IDE as a `.ko` on ext2” as unfinished P8.

---

## Guiding principles

1. **Linux ABI first** — numbers, structs, errno, ELF, auxv, TLS (`arch_prctl`), process model.
2. **Correct layering** — MM → isolation → schedule → threads → signals/futex → **VFS → modules**.
3. **Validation without becoming BusyBox-shaped** — small Rust/C test programs + selective musl/BusyBox smoke.
4. **Rust kernel discipline** — `no_std`, explicit unsafe boundaries, module ABI that does not require forever-unstable Rust dylibs (prefer C-compatible kernel API for modules, modules themselves can be Rust or C).

---

## Phase map (dependency order)

```text
  P1  Per-process address spaces (CR3, page-table clone)
        │
        ▼
  P2  Real fork/exec/exit (COW or full copy) — drop parent-image snap hacks
        │
        ▼
  P3  Preemptive scheduler + kernel context switch
        │
        ├──────────────────────────────┐
        ▼                              ▼
  P4  Threads (clone, TID, TLS)   P5  Signals (delivery, masks)
        │                              │
        └──────────────┬───────────────┘
                       ▼
  P6  Futex + clear_child_tid + exit_group semantics
                       │
                       ▼
  P7  VFS / device model (ops tables, char/block)
                       │
                       ▼
  P8  Kernel modules (loader, symbols, init/exit, refcount)
                       │
                       ▼
  P9  Broader Linux surface (net optional, more FS, SMP later)
```

BusyBox suite stays a **regression gate** after each phase — not the work queue.

---

## Phase 1 — Per-process address spaces (foundation)

**Status:** **done** (see [MM.md](MM.md)). Private CR3 on fork; shared CR3 only with `CLONE_VM`.

**Why first:** every Linux process assumes private page tables. Shared AS is only for threads with `CLONE_VM`.

### Deliverables

- Each PCB (or `mm_struct`) holds **`cr3` / PML4 phys**.
- Kernel mapped the same in every address space (higher-half or fixed identity kernel window — pick one and freeze in ABI).
- `switch_mm(prev, next)` loads CR3 on context switch.
- User pages allocated as private frames (not “overwrite identity and pray”).
- Page fault handler path that can grow stacks / demand-zero later (minimal OK).

### Exit criteria

- Two user processes cannot corrupt each other’s heap by normal writes.
- `execve` tears down only **that** process’s user maps.
- Parent image snapshot for fork **deleted** (or only used as temporary debug).

### Suggested design notes

- Start with **full page-table copy** on fork (simple, correct). COW later.
- Keep a permanent kernel mapping so syscall entry never walks user-only tables for kernel code/data.
- Document user VA layout (ET_EXEC load, stack top, mmap arena, vsyscall if any).

---

## Phase 2 — Process lifecycle that matches Linux

**Status:** **mostly done** — `fork`/`execve`/`exit`/`wait4` on private mm; Ready + schedule; no image snap.
`vfork` still ENOSYS (use `fork`). Reparent/WNOHANG polish may remain.

### Deliverables

- `fork` / `vfork` / `execve` / `exit` / `wait4` on **private mm**. ✅ except real `vfork`
- Zombies, reparent to init, `WNOHANG`. 🟡 basic zombies/wait
- Child not run to completion inside `sys_fork` — Ready + scheduler. ✅

### Exit criteria

- Shell can `fork+exec` without snapshot buffers. ✅
- Concurrent parent/child do not share writable pages unless COW/`CLONE_VM`. ✅

---

## Phase 3 — Scheduler (required for threads)

**Why:** Linux threads are schedulable entities. Without a real scheduler, `clone` is only “another PCB you run nested.”

**Status (2026-07-31):** **Phase 3 done** — RR Ready queue; timer IRQ user→user preempt via `TrapFrame`; per-process kstacks (TSS.RSP0 ≠ nest stack); sticky `entered_via_nest` + `resume_user_trap`; nest policy depth≤1 for IRQ (depth≥2 cooperative). Verify: `munux> preempttest` (A–G PASS) + forktest/busybox. No in-kernel preemption (not required for user threads).

### Deliverables

- Run queue of **tasks** (process or thread). ✅ pick_ready RR
- Preemption on timer IRQ (save full user frame). ✅ user→user (incl. under nest)
- Kernel context switch: save/restore callee-saved + stack pointer; per-task **kernel stack**. 🟡 user stacks done; no in-kernel preemption
- States: Running / Ready / Sleeping / Zombie (you already have names). ✅
- `schedule()`, `wake_up()`, sleep on wait queues (even if only used by `wait`/`futex` later). ✅ `take_ready` / `wake_up` / `try_preempt`

### Exit criteria

- Two CPU-bound user loops make progress without cooperative yield. ✅ `preempttest` G
- Timer tick can switch tasks safely (no stack smash, correct CR3 + TLS). ✅

### Nice (optional in P3)

- Use existing `nice` field for simple priority; not required for correctness.

---

## Phase 4 — Threads (north star #1)

**Status (2026-08-02):** **4a–4c done** — `tid`/`tgid`, `gettid`, `clone` (VM/FILES/THREAD/settids),
shared-mm + shared-FD refcounts, `exit_group` kills thread group. Smoke: `clonetest`.
Join path needs Phase 6 futex (done in first slice).

Linux model (simplified):

| Concept | Linux | munux target |
|---------|--------|--------------|
| Process | thread group, `tgid` = pid of leader | `tgid` on task |
| Thread | task with own `tid`, may share mm/fs/files | `clone` flags |
| TLS | `arch_prctl` / `set_thread_area`; per-task `fs_base` | already partly there |
| Sync | futex | P6 |

### Deliverables

- Syscall **`clone`** (and eventually `clone3`) with at least:
  - `CLONE_VM` — share address space
  - `CLONE_FS` / `CLONE_FILES` — share cwd / FD table (refcounted)
  - `CLONE_THREAD` — same thread group
  - `CLONE_SIGHAND` — shared signal handlers (with P5)
  - `CLONE_CHILD_CLEARTID` / `CLONE_PARENT_SETTID` — musl/nptl needs these
- Separate **TID** vs **TGID** (`gettid` ≠ always `getpid` for threads).
- Per-thread user stack (caller provides via `clone` stack arg).
- Per-thread kernel stack + `fs_base`.
- `exit` kills one thread; **`exit_group`** kills the thread group.
- FD tables and mm become **refcounted shared objects**, not flat per-slot copies only.

### Exit criteria

- musl-linked tiny program: `pthread_create` + join (may need futex P6 for full join).
- `gettid` differs across threads; `getpid` same for `CLONE_THREAD`.
- Stress: N threads write disjoint stacks; no cross-corruption.

### Non-goals for first thread milestone

- Full POSIX cancellation, robust mutex lists, NUMA — later.

---

## Phase 5 — Signals (parallelizable with late P4)

**Status (2026-08-02):** **Phase 5 done (practical slice)** — kill/tkill/tgkill,
rt_sigaction/procmask/sigreturn, default terminate + user handlers (stack frame +
restorer), TTY Ctrl-C → SIGINT (prefer current job), shell SIG_IGN for INT/QUIT.
Smoke: `signaltest`. Not Linux-complete: full siginfo/ucontext, SA_NODEFER, RT signals.

### Deliverables

- Real pending/blocked masks; deliver on return-to-user. ✅ basic
- `rt_sigaction`, `rt_sigprocmask`, `rt_sigreturn`, `kill`, `tkill`/`tgkill`. ✅
- Default actions (terminate, ignore); minimal handler frame on user stack. ✅

### Exit criteria

- `kill(pid, SIGTERM)` ends process. ✅
- Thread-directed signals with `tgkill`. ✅
- User handler + return. ✅
- Interactive Ctrl-C stops foreground job, not empty shell. ✅

---

## Phase 6 — Futex (makes threads actually usable)

**Status (2026-08-02):** **6a–6c practical done** — WAIT/WAKE/REQUEUE/CMP_REQUEUE (+PRIVATE),
bitset aliases, relative timeout → ETIMEDOUT, `clear_child_tid` + auto-reap non-leader
threads, wait runs Ready **children only**, nest-safe spurious wake. Smoke: `futextest`
(timeout + join + mutex + requeue). Remaining: PI/robust, absolute timedwait, musl soak.

### Deliverables

- `futex` wait/wake (and preferably `FUTEX_PRIVATE`). ✅
- `set_tid_address` + clear TID word + wake on thread exit (musl join). ✅
- Wait queues keyed by user VA (+ mm). ✅
- Requeue + relative timeout. ✅ practical
- Non-leader threads freed on exit (join via futex). ✅

### Exit criteria

- `pthread_mutex` / `pthread_join` work on musl. 🟡 path smoke via `futextest` (not full musl)
- No busy-spin required for correctness. ✅ (cooperative schedule while waiting)

---

## Phase 7 — VFS + device model (prepares modules)

**Status (2026-08-07):** **done (practical)** — 7a–7d.

- fops + mounts (ext2 / ram / proc) + Linux-like dir visibility  
- chrdev (`null`/`zero`/`hda`) + blkdev `hda` (ext2 I/O via blockdev)  
- path mutations via **vops** (mkdir/unlink/rmdir/rename/link)  
- **pipe** / **dup** / **dup2** (cooperative)  
- Not Linux-complete: full dentry cache, `mount`/`umount` syscalls, rich inode cache  

### Deliverables

- VFS objects: `file_operations` + `FileData` + mounts + vops. ✅ practical  
- Mount table. ✅ `/` ext2, `/ram` ramfs, `/proc` proc  
- Char/block device registration. ✅  
- IDE/ext2 behind ops. ✅  

### Exit criteria

- Open/read/write go through VFS ops. ✅  
- A second FS can be registered without rewriting syscalls. ✅  
- Mutations + pipes for real userspace plumbing. ✅ practical

---

## Phase 8 — Kernel modules (north star #2)

**Status (2026-08-07):** **done (8a–8c).** Phase 8 is **closed**.

- MNX1 container **and** ELF64 ET_REL `.ko` (not mainline vermagic / GPL ksymtab)
- Export table + PC32 trampolines so `call munux_*` from high-heap images works
- Kernel shell + **userspace** `/bin/insmod|rmmod|lsmod` via Linux syscalls
- `hello.mnx` / `hello.ko`; `echo.mnx` / `echo.ko` → `/dev/echo` + EBUSY unload
- `/proc/modules`

Vermagic, `depmod`, signing, and mainline `.ko` ABI were **non-goals**, not leftover P8 work.

### What “Linux-compatible modules” means for munux

Aim for **conceptual compatibility** with Linux LKMs, not binary `.ko` from mainline Linux (that requires identical kernel ABI, GPL symbols, version magic — unrealistic early).

**Target:**

1. Load an **ELF relocatable object** from the filesystem (or initrd).
2. Resolve symbols against a kernel **export table** (`EXPORT_SYMBOL`-like).
3. Call `init()`; on unload call `exit()` if refcount allows.
4. Modules register drivers/FS via the VFS/device API from P7.

### Deliverables

| Piece | Description | Status |
|-------|-------------|--------|
| `struct module` | name, state, refcount, init/exit, section pointers | ✅ |
| Export table | C ABI: printk, register/unregister chrdev | ✅ |
| Loader | **MNX1** + **ELF64 ET_REL** `.ko` (PC32 trampolines) | ✅ 8c |
| Admin | kernel shell + `init_module`/`delete_module`/`finit_module` + `/bin/*` | ✅ |
| Memory | kmalloc heap + dual-map into kernel CR3 | ✅ workaround |
| Safety | unload only if refcount 0; chrdev open holds ref | ✅ |

### Module authoring (Rust)

Recommended approach:

- Define a **C ABI** kernel API (`extern "C"` headers or `#[repr(C)]` + bindgen-friendly surface).
- Modules compiled as separate **`cdylib` / relocatable** crates with `panic=abort`, `no_std`.
- Avoid depending on unstable Rust crate ABI between kernel and module.

### Exit criteria

- Load `hello.ko` (or `hello.mnx`) that prints on init and unloads cleanly. ✅ both
- Load a **char device module** that creates `/dev/echo` and works with `open`/`read`/`write`. ✅ `.mnx` + `.ko`
- ~~Built-in IDE driver shipped only as a module~~ **withdrawn** — see below.

### Why “IDE as a loadable module” is not a P8 exit item

**IDE/ATA** is the **boot disk** protocol (`hda` → ext2 `/` → `/lib/modules`).  
`insmod` reads files **from that disk**. If the IDE driver were *only* a `.ko` on ext2:

1. You need IDE to read `/lib/modules/ide.ko`
2. You need `ide.ko` to have IDE  
→ chicken-and-egg.

Linux only loads a disk driver as a module when there is **initramfs** (or the driver is built-in, `=y`). munux has no initrd and no second boot device. **`hda` must stay linked into `kernel.bin`.** Echo-as-module already proves the VFS/device registration story; the root disk driver is a **boot** problem, not a module-loader gap.

Optional later (not P8, not a Phase 9 gate):

- Keep IDE **built-in** (correct default).
- Optionally put `register_blkdev` on the C export table so a *second* block driver could be a module — API hygiene only.
- Initrd + optional ATA `.ko` is **boot architecture**, same bucket as SMP/ACPI.

### Non-goals (not “unfinished P8”)

- Livepatch, module signing, full mainline vermagic, dependency trees (`depmod`)
- Binary compatibility with Linux `.ko` files
- Unloading `hda` while `/` is mounted on it

---

## Phase 9 — Broaden Linux compatibility (ongoing)

Prioritize by **kernel completeness**, not applet count:

| Priority | Area | Why |
|----------|------|-----|
| High | `readlink`/`symlink`, `statx`, `execveat`, `prctl` | Real userspace tooling |
| High | File-backed `mmap`, correct ELF loader | Dynamic linkers later |
| Medium | `epoll`/`select`, `pipe` polish | Evented programs |
| Medium | `mount`/`umount`, ramfs, better `/proc`/`sys` | Module-loaded FS |
| Later | Sockets / TCP | Only if networking is a goal |
| Later | SMP, ACPI, initrd / optional disk `.ko` | Boot + scale-out — not unfinished P8 |

Syscall coverage % is a **metric**, not a milestone by itself.

---

## Validation strategy (not “all BusyBox”)

| Layer | What to run |
|-------|-------------|
| Unit / kernel tests | Page-table isolate test; clone stack test; futex wait/wake |
| ABI smoke | Tiny static musl programs per feature (`pthread_create`, `mmap`, …) |
| Regression | Keep **strict BusyBox suite** (~48 cases) green after each phase |
| Module | Load/unload hello + chardev under qemu-connect headless |
| Avoid | 300-applet zero-arg BusyBox marathon as planning input |

---

## Suggested near-term work order

| # | Milestone | Outcome | Status |
|---|-----------|---------|--------|
| **M1** | **mm_struct + per-process CR3** | Isolation foundation | ✅ done |
| **M2** | **fork copies page tables; drop image snap** | Honest processes | ✅ done |
| **M3** | **Timer preemption + schedule()** | Real multitasking | ✅ done |
| **M4** | **clone + TID + shared mm/files** | Threads exist | ✅ done |
| **M5** | **signals + futex + clear_child_tid** | kill/handlers/Ctrl-C; join path | ✅ practical |
| **M6** | **VFS ops + register_chrdev** | Pluggable drivers | ✅ P7 practical |
| **M7** | **module loader + EXPORT_SYMBOL + hello + echo chrdev** | Loadable kernel code | ✅ P8a–8c (MNX1 + ET_REL) |

Everything else (more syscalls, net, polish) hangs off this spine.

---

## What to stop optimizing for

- One-off applets that only need another ENOSYS stub.
- Growing shared-AS workarounds (image snap is **deleted** — keep it that way).
- Treating “% of Linux syscalls implemented” as the main KPI.
- Reopening P8 to turn the **root IDE disk** into a `.ko` on that same disk.

What **to** keep:

- Linux syscall numbers and struct layouts.
- Headless qemu-connect tests + focused smokes (`preempttest`, `clonetest`, `signaltest`, `futextest`).
- Small, reviewable phases with clear exit criteria.

---

## Risk register

| Risk | Mitigation |
|------|------------|
| Identity-map kernel forever blocks high-half / modules | Plan kernel VA when modules need it; identity is OK for P7 start |
| Nest depth ≥ 2 stays cooperative | Document; only deepen nest preempt with careful testing |
| Rust module ABI fragility | C ABI boundary for all module exports |
| Too many goals at once | P8 is closed; next epic is Phase 9 — not BusyBox stubs or IDE-as-`.ko` |
| BusyBox regressions demoralize | Gate: suite + focused smokes after each M*, not drive design |

---

## Success definition (12–18 month horizon, rough)

munux is a **Linux-compatible teaching/research kernel in Rust** when:

1. Static musl programs use **fork, exec, threads (pthread), futex, mmap, files** as on Linux.
2. Processes are **memory-isolated**.
3. The kernel can **load and unload a driver module** that registers a device under VFS.
4. Syscall surface grows deliberately behind that architecture — not ahead of it.

**Today:** (1) partial (threads + basic futex/signals; no full musl pthread),
(2) yes, (3) **yes** (`echo.ko` / `echo.mnx` chardev), (4) intentional.

---

## Immediate recommendation (handoff for next session)

**Phase 8 is complete.** Do not reopen it for “make IDE a `.ko` on ext2.”

**Start Phase 9** — pick one high-value Linux-surface slice:

1. **File-backed `mmap` + ELF polish** (unlocks more musl/dynamic later)
2. **`readlink` / `symlink` / `statx`** (real tooling)
3. **`epoll`/`select`** (evented programs; pipes already exist)

Keep qemu-connect smokes green: `signaltest`, `clonetest`, `futextest`, `echotest`,
`insmod …/hello.ko`, `preempttest`.

See [SMOKE_MODULE.md](SMOKE_MODULE.md) · [SMOKE_VFS.md](SMOKE_VFS.md).
