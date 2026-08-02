# munux roadmap — Linux-compatible kernel in Rust

**Last updated:** 2026-08-02 (aligned with README; foundation P1–P6 practical slices done).

**Goal:** a **Linux x86_64 ABI–compatible** kernel written in Rust, not “run every BusyBox applet.”

BusyBox / musl binaries are **compatibility probes** (does `fork`/`clone`/`mmap`/ELF load match Linux?). They are not the product definition.

**Related docs:** [README](../README.md) · [ABI](ABI.md) · [MM](MM.md) · [SYSCALL_COMPARE](SYSCALL_COMPARE.md) · [SMOKE_PREEMPT](SMOKE_PREEMPT.md) · [SMOKE_CLONE](SMOKE_CLONE.md) · [SMOKE_SIGNAL](SMOKE_SIGNAL.md) · [SMOKE_FUTEX](SMOKE_FUTEX.md) · [BusyBox suite](BUSYBOX_SUITE_REPORT.md)

**North stars:**

1. **Thread support** (Linux `clone` / TID model / futex) — **foundation in place**
2. **Kernel modules** (loadable objects, symbol export, init/exit) — **next major epic**

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
| Futex | WAIT/WAKE + clear_child_tid wake | OK basic join |
| FS | **VFS 7a**: ops tables, mounts (ext2 + ramfs), open/read/write via fops; `/proc` still ad-hoc | Partial — deepen for modules |
| Drivers | Compile-time + **chrdev** (`null`/`zero`); IDE still behind ext2 | Partial |
| Modules | None | **Missing (P8)** |

**Implication:** thread foundation + **VFS first slice** landed. Next: finish P7 polish → **loadable modules (P8)**.

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

**Status (2026-08-02):** **7a first slice** — `file_operations` table, mount registry,
`register_chrdev`, open/read/write via VFS (`fs/vcore.rs` + FD layer). Root **ext2** +
**ramfs** (`/ram/*`) + **`/dev/null`** / **`/dev/zero`**. Kernel shell: `vfs`.
Not yet: full dentry cache, block-device ops for IDE, unify `/proc` behind VFS.

### Deliverables

- VFS objects: `super_block`, `inode`, `dentry`/`path`, `file` with `file_operations`.
  🟡 `FileData` + `FileOperations` + mounts/chrdev (inode/dentry cache later)
- Mount table (even if only one root). ✅ multi-mount (`/` ext2, `/ram` ramfs)
- Char/block device registration (`register_chrdev` style). ✅ chrdev; block later
- IDE/ext2 become “built-in modules” behind the same ops as loadable ones. 🟡 ext2 via fops

### Exit criteria

- Open/read/write go through VFS ops, not `ext2_*` scattered in syscalls. ✅ FD path
- A second FS (e.g. ramfs) can be registered without rewriting syscalls. ✅ `/ram`

---

## Phase 8 — Kernel modules (north star #2)

### What “Linux-compatible modules” means for munux

Aim for **conceptual compatibility** with Linux LKMs, not binary `.ko` from mainline Linux (that requires identical kernel ABI, GPL symbols, version magic — unrealistic early).

**Target:**

1. Load an **ELF relocatable object** from the filesystem (or initrd).
2. Resolve symbols against a kernel **export table** (`EXPORT_SYMBOL`-like).
3. Call `init()`; on unload call `exit()` if refcount allows.
4. Modules register drivers/FS via the VFS/device API from P7.

### Deliverables

| Piece | Description |
|-------|-------------|
| `struct module` | name, state, refcount, init/exit, section pointers |
| Export table | kernel symbols with names + addresses (+ optional CRC/version later) |
| Loader | parse ELF ET_REL (or simplified container), apply relocations (x86_64) |
| Syscalls or admin interface | start with **kernel shell command** or `init_module`/`delete_module` stubs |
| Memory | module code/data in kernel VA (dedicated heap or `vmalloc`-like region) |
| Safety | unload only if refcount 0; no use-after-free of ops pointers |

### Module authoring (Rust)

Recommended approach:

- Define a **C ABI** kernel API (`extern "C"` headers or `#[repr(C)]` + bindgen-friendly surface).
- Modules compiled as separate **`cdylib` / relocatable** crates with `panic=abort`, `no_std`.
- Avoid depending on unstable Rust crate ABI between kernel and module.

### Exit criteria

- Load `hello.ko` (or `hello.mnx`) that prints on init and unloads cleanly.
- Load a **char device module** that creates `/dev/echo` and works with `open`/`read`/`write`.
- Built-in IDE driver can later be recompiled as a module without API rewrite.

### Non-goals early

- Livepatch, module signing, full mainline vermagic, dependency trees (`depmod`) — phase 9+.

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
| Later | SMP, ACPI, userspace drivers | Scale-out |

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
| **M6** | **VFS ops + register_chrdev** | Pluggable drivers | 🟡 7a started |
| **M7** | **module loader + EXPORT_SYMBOL + hello module** | Loadable kernel code | planned |

Everything else (more syscalls, net, polish) hangs off this spine.

---

## What to stop optimizing for

- One-off applets that only need another ENOSYS stub.
- Growing shared-AS workarounds (image snap is **deleted** — keep it that way).
- Treating “% of Linux syscalls implemented” as the main KPI before modules land.

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
| Too many goals at once | Active epic: **VFS → modules** (not more BusyBox stubs) |
| BusyBox regressions demoralize | Gate: suite + focused smokes after each M*, not drive design |

---

## Success definition (12–18 month horizon, rough)

munux is a **Linux-compatible teaching/research kernel in Rust** when:

1. Static musl programs use **fork, exec, threads (pthread), futex, mmap, files** as on Linux.
2. Processes are **memory-isolated**.
3. The kernel can **load and unload a driver module** that registers a device under VFS.
4. Syscall surface grows deliberately behind that architecture — not ahead of it.

**Today:** (1) partial (threads foundation + basic futex/signals), (2) yes, (3) not started, (4) intentional.

---

## Immediate recommendation

**Continue Phase 7:** deepen VFS (dentry/path cache, `/proc` behind ops, block-dev ops for IDE), then **P8 modules**.

Thread foundation (P1–P6) + VFS 7a are landed. Next architecture unlock is **modules** on top of ops tables.

Conversation-sized follow-ups:

1. Introduce `file_operations` / `inode` / open path that syscalls go through.  
2. Move ext2 + `/proc` behind those ops (behavior unchanged).  
3. `register_chrdev`-style hook (even if only for a built-in null/zero).  
4. Keep focused smokes green after each step.
