# munux roadmap — Linux-compatible kernel in Rust

**Last updated:** 2026-08-09 (P11b PTY pair).

**Goal:** munux is a **Linux x86_64 kernel written in Rust**. The destination is to **install a Linux desktop environment and use the machine like a Linux system**.

Think **clang vs gcc**: same job, different implementation. Internals may differ (language, VFS, scheduler, module container) as long as **userspace gets the same results** — same syscall numbers and structs, same process/thread/file/mmap/ELF semantics, same programs, same desktop.

BusyBox / static musl binaries are **probes and regression tests**, not the product. Syscall coverage is a **progress metric** toward real Linux userspace (glibc, dynlink, a DE), not a vanity checklist and not something to stop at “architecture only.”

**Related docs:** [README](../README.md) · [ABI](ABI.md) · [MM](MM.md) · [SYSCALL_COMPARE](SYSCALL_COMPARE.md) · [SMOKE_PREEMPT](SMOKE_PREEMPT.md) · [SMOKE_CLONE](SMOKE_CLONE.md) · [SMOKE_SIGNAL](SMOKE_SIGNAL.md) · [SMOKE_FUTEX](SMOKE_FUTEX.md) · [SMOKE_VFS](SMOKE_VFS.md) · [SMOKE_MODULE](SMOKE_MODULE.md) · [LINUXKPI](LINUXKPI.md) · [BusyBox suite](BUSYBOX_SUITE_REPORT.md)

**North stars:**

1. **Linux userspace results** — eventually a desktop (display + input + shell + apps) on munux, same as on Linux.
2. **Architecture spine (done)** — isolated processes, joinable threads, loadable drivers. That was the *path*, not the finish line.
3. **Grow the ABI on purpose** until a real distro userspace + DE works.
4. **Linux driver sources** (linuxkpi) — compile upstream `.c` against munux headers; not distro `.ko` binaries. See [LINUXKPI.md](LINUXKPI.md).

---

## 0. Where we are today (honest baseline)

| Area | Current state | Blocker for modules? |
|------|---------------|----------------------|
| Arch | x86_64 long mode, `syscall`/`sysret`, GDT/TSS/IDT | OK |
| Syscall ABI | Linux numbers; growing surface | Partial — OK to grow |
| Memory | **Per-process CR3** + `clone_mm`; identity kernel window | OK for threads; high-half later |
| Processes | PCB tid/tgid; fork private mm; wait/exit_group | OK |
| Scheduling | Timer user→user preempt (`TrapFrame`); nest policy depth ≤ 1 | OK for user threads |
| Threads | **`clone`/`clone3`**, shared mm/files, gettid/tgid, SETTLS | OK (no full nptl `pthread_create` soak yet) |
| Signals | kill/tgkill, masks, handlers, rt_sigreturn, Ctrl-C | OK for practical use |
| Futex | WAIT/WAKE/REQUEUE + timeout + clear_child_tid | OK basic join |
| FS | **VFS P7 practical**: fops, mounts, proc, mutations, pipes | OK for modules |
| Drivers | chrdev + blkdev; **IDE `hda` is built-in** (root disk) | OK — must stay built-in |
| Modules | **P8a–8c done**: MNX1 + ELF ET_REL `.ko`, `/dev/echo` | Not blocked |

**Implication:** P7 + **P8 complete** (hello/echo as `.mnx` and `.ko`).  
**Current epic: Phase 9** (broader Linux surface toward a real userspace). Later epics (dynlink, net, graphics, install) are **in scope** for the desktop goal. Do **not** treat “IDE as a `.ko` on ext2” as unfinished P8.

---

## Guiding principles

1. **Same results as Linux** — numbers, structs, errno, ELF, auxv, TLS, process/thread/file/mmap semantics. Userspace (BusyBox today, glibc + a DE later) should not care that the kernel is Rust.
2. **Different internals are fine** — not a Linux source clone, not mainline `.ko` binary compat, not “do it the Linux way” unless that is the cheapest path to the same result.
3. **Correct layering** — MM → isolation → schedule → threads → signals/futex → **VFS → modules** → remaining Linux surface → desktop stack.
4. **Validate with real userspace** — focused smokes + BusyBox regression now; grow to musl/glibc dynlink, then a desktop. Do not let applet-count become the work queue, but do not stop short of what a DE needs.
5. **Rust kernel discipline** — `no_std`, explicit unsafe boundaries, module ABI that does not require forever-unstable Rust dylibs (prefer C-compatible kernel API for modules, modules themselves can be Rust or C).

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
          ┌────────────┴────────────┐
          ▼                         ▼
  P9  Linux userspace surface    LK  linuxkpi (compile Linux .c drivers)
      (syscalls, mmap, …)  ←now     L0 loader → L1 printk → L2 cdev → virtio
          │                         │
          └────────────┬────────────┘
                       ▼
  P10 Dynamic linking + ELF file maps (musl/glibc-class process)
                       │
                       ▼
  P11 TTYs / PTYs / termios / job control
                       │
                       ▼
  P12 Networking (sockets → TCP/IP → virtio-net)
                       │
                       ▼
  P13 Graphics + input (fb/KMS, evdev → X11/Wayland)
                       │
                       ▼
  P14 Installable Linux desktop (packages + DE + daily use)
```

BusyBox suite stays a **regression gate** after each phase. It is not the destination.

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

## Phase 4 — Threads (spine)

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

## Phase 8 — Kernel modules (spine)

**Status (2026-08-07):** **done (8a–8c).** Phase 8 is **closed**.

- MNX1 container **and** ELF64 ET_REL `.ko` (not mainline vermagic / GPL ksymtab)
- Export table + PC32 trampolines so `call munux_*` from high-heap images works
- Kernel shell + **userspace** `/bin/insmod|rmmod|lsmod` via Linux syscalls
- `hello.mnx` / `hello.ko`; `echo.mnx` / `echo.ko` → `/dev/echo` + EBUSY unload
- `/proc/modules`

Vermagic, `depmod`, signing, and **prebuilt distro `.ko` files** remain out of P8.
**New epic [LINUXKPI](LINUXKPI.md):** compile Linux **driver sources** against munux `include/linux/*.h` (clang vs gcc for drivers).

### What “Linux-compatible modules” means for munux

**P8 (done):** conceptual LKM lifecycle — ELF ET_REL + tiny `munux_*` exports + NASM hello/echo.

**Next (linuxkpi):** same lifecycle, **Linux C API** so upstream driver `.c` files build and run. Not Ubuntu’s prebuilt `.ko`.

**Target:**

1. Load an **ELF relocatable object** from the filesystem (or initrd). ✅
2. Resolve symbols against a kernel **export table** (`EXPORT_SYMBOL`-like). ✅ munux names; Linux names in LK
3. Call `init()`; on unload call `exit()` if refcount allows. ✅
4. Modules register drivers/FS via the VFS/device API from P7. ✅ tiny fops; Linux `file_operations` in L2

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
- Binary compatibility with **distro** Linux `.ko` files (Ubuntu/Fedora blobs)
- Unloading `hda` while `/` is mounted on it

Source-compatible Linux drivers are **not** a P8 leftover — they are epic **LK** ([LINUXKPI.md](LINUXKPI.md)).

---

## Phase 9 — Broaden Linux compatibility (ongoing)

P9 is the **current** epic: grow the Linux syscall/VFS/mmap surface so more real userspace works. That surface is required for dynlink, a desktop, and everything after.

Prioritize by **what Linux userspace (and later a DE) actually needs**, not by “implement syscall N next” and not by BusyBox applet count.

| Priority | Area | Why |
|----------|------|-----|
| High | ~~`readlink`/`symlink`/`statx`~~ ✅ P9a | Real userspace tooling |
| High | ~~File-backed `mmap`~~ ✅ P9b snapshot `MAP_PRIVATE` | True COW / `MAP_SHARED` writeback later |
| High | ~~ELF loader using file maps~~ ✅ P9e | Stream `PT_LOAD` from inode; no 2 MiB kernel copy |
| High | ~~`execveat`, `prctl`~~ ✅ P9d | Tooling / process control |
| Medium | ~~`epoll`/`select`~~ ✅ P9c level-triggered | `ppoll` sigmask / ET epoll later |
| Medium | `mount`/`umount`, ramfs, better `/proc`/`sys` | Module-loaded FS + install story |
| Medium | `vfork`, richer `clone3` (set_tid/cgroup), waitid | glibc/musl spawn paths |
| Later in P9 | COW fork, demand paging, shared-anon mmap | `MAP_SHARED` file writeback ✅ P9e (no EOF-extend / COW) |

Syscall coverage % (**95 / 385 ≈ 24.7%** today) is a **progress metric** toward the desktop, not a vanity KPI and not a reason to stop. Implement what userspace needs with **Linux semantics**; skip Linux-internal-only or obsolete calls until something actually requires them.

---

## Epic LK — Linux driver sources (linuxkpi)

**Plan:** [LINUXKPI.md](LINUXKPI.md). **L0–L5 + virtio-net done.** Parallel to P9 (sockets next for real userspace net).

Compile Linux **`.c` drivers** against munux-owned `include/linux/*.h`. Implement that C API in Rust (`extern "C"`). Do **not** load prebuilt Ubuntu `.ko` files.

| Slice | Result | Status |
|-------|--------|--------|
| **L0** | gcc ET_REL relocates (GOT/PC32, bigger limits) | ✅ |
| **L1** | `printk` / `kmalloc` / `module_init` — gcc `hello_c.ko` loads | ✅ |
| **L2** | Linux `file_operations` / misc — `echo_c.ko` + `echotest` PASS | ✅ |
| **L3** | spinlock / completion / `request_irq` — `irqtest.ko` | ✅ |
| **L4** | `ioremap` + PCI scan / `pci_register_driver` — `vprobe.ko` | ✅ |
| **L5** | virtio-blk → `/dev/vda` (`vdatest: PASS`) | ✅ |

MNX1 + NASM modules stay until L2 is green, then freeze.

---

## Phases 10–14 — Path to a Linux desktop

These are **in scope**. Internals can differ from Linux; the **result** must not.

| Phase | Result we are aiming for | Notes |
|-------|--------------------------|-------|
| **P10** | Dynamically linked musl/glibc binaries run | P10a–d ✅ interp + `ET_DYN` + **glibc `hello_dyn`** + `clone3`/`CLONE_SETTLS`; full `pthread_create` still open |
| **P11** | Real terminals and job control | P11a–b ✅ session/pgrp + console tty + **PTY pair**; still need n_tty / `SIGTTOU` |
| **P12** | Networking works | virtio-net + ICMP ping ✅; still need `socket`/`bind`/`connect` for userspace |
| **P13** | Graphics + input | Framebuffer or KMS/DRM + evdev/mice/keyboard in Linux ABI form; then Xorg or a Wayland compositor |
| **P14** | **Install and use a Linux desktop** | Package a userspace (or boot a distro rootfs), start a display manager / DE, use it like Linux |

**OK to differ from Linux:** language (Rust), module container (MNX1 + our ET_REL `.ko` vs mainline vermagic), scheduler/VFS internals, missing Linux-only debugfs, no binary compat with Ubuntu `nvidia.ko`.

**Not OK to differ:** userspace-visible ABI and behavior that a DE, libc, or package manager relies on.

SMP, ACPI, initrd, and “optional disk `.ko` after initrd” are **boot/scale** work that a serious desktop install will eventually want — not leftover P8.

Linux **driver sources** (virtio-blk/net, later GPU) go through [LINUXKPI.md](LINUXKPI.md) (L0–L5), in parallel with P9.

---

## Validation strategy

| Layer | What to run |
|-------|-------------|
| Unit / kernel tests | Page-table isolate test; clone stack test; futex wait/wake |
| ABI smoke | Tiny static musl programs per feature (`pthread_create`, `mmap`, …) |
| Regression | Keep **strict BusyBox suite** (~48 cases) green after each phase |
| Module | Load/unload hello + chardev under qemu-connect headless |
| Later | Dynlinked hello-world → glibc tools → X/Wayland smoke → DE session |
| Avoid | 300-applet zero-arg BusyBox marathon as the *only* planning input |

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

Everything else (more syscalls, net, graphics, desktop install) hangs off this spine — that “everything else” **is** the product after P8.

---

## What to stop optimizing for

- One-off applets that only need another ENOSYS stub.
- Growing shared-AS workarounds (image snap is **deleted** — keep it that way).
- Treating munux as a **teaching kernel that stops at threads + modules**.
- Reopening P8 to turn the **root IDE disk** into a `.ko` on that same disk.
- Copying Linux *internals* (or mainline `.ko` ABI) when a different implementation already yields the same userspace result.

What **to** keep:

- Linux syscall numbers, struct layouts, and **observable** behavior.
- Syscall / ABI coverage as a **progress metric** toward a desktop.
- Headless qemu-connect tests + focused smokes (`preempttest`, `clonetest`, `signaltest`, `futextest`).
- Small, reviewable phases with clear exit criteria.

---

## Risk register

| Risk | Mitigation |
|------|------------|
| Identity-map kernel forever blocks high-half / modules | Plan kernel VA when modules need it; identity is OK for P7 start |
| Nest depth ≥ 2 stays cooperative | Document; only deepen nest preempt with careful testing |
| Rust module ABI fragility | C ABI boundary for all module exports |
| Too many goals at once | One epic at a time (now P9); desktop is the *destination*, not this week’s slice |
| BusyBox regressions demoralize | Gate: suite + focused smokes after each M*; do not let applets replace the desktop goal |

---

## Success definition

munux **succeeds** when you can **install a Linux desktop environment and use the system like Linux** — same apps, same results, kernel written in a different language (clang vs gcc).

Milestones on that path:

| Horizon | Result | Status |
|---------|--------|--------|
| Spine | Isolated processes, joinable threads, loadable drivers | ✅ P1–P8 |
| Probe userspace | Static musl / BusyBox: fork, exec, pthread path, futex, mmap, files | 🟡 partial (no full musl/glibc pthread soak) |
| Real userspace | Dynamically linked glibc/musl, PTYs, net, graphics/input | 🟡 P10a–d + P11a–b in; P12–P13 still open |
| **Desktop** | Install a DE and use it as a daily Linux machine | ❌ P14 — **the product goal** |

**Today:** spine is in; ~26% of Linux x86_64 syscalls dispatched; BusyBox/static musl/glibc hello are probes. That is **early** on the path to a desktop, not a reason to redefine the goal.

---

## Immediate recommendation (handoff for next session)

**Phase 8 is complete.** Do not reopen it for “make IDE a `.ko` on ext2.”

**P11b (PTY pair) landed.** Smoke: `ptytest`. Next: n_tty polish or P12 BSD sockets so userspace can use the NIC.

**P9 leftover:** COW fork / demand paging as needed by dynlink.

**LK (drivers):** [LINUXKPI.md](LINUXKPI.md) — L0–L5 + virtio-net (ICMP ping). Next: BSD sockets (P12) so userspace can use the NIC.

Do not reopen P8 for MNX1 features. New modules are linuxkpi C.

Keep qemu-connect smokes green: `signaltest`, `clonetest`, `futextest`, `echotest`,
`insmod …/hello.ko`, `preempttest`.

See [SMOKE_MODULE.md](SMOKE_MODULE.md) · [SMOKE_VFS.md](SMOKE_VFS.md).
