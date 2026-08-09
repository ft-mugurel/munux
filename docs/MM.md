# munux memory layout

**Status:** living layout freeze (private mm + shared-mm threads).  
**Last updated:** 2026-08-07 (P7–P8c done; layout unchanged).

Higher-half is **not** required yet. The kernel stays in the **identity window** until a later milestone.

---

## Rules

1. **Kernel mapping** is the same in every process address space (shared page-table leaves or shared PML4 entries for the kernel window).
2. **User pages** are **private frames** per mm (fork copies USER leaves). Threads with `CLONE_VM` **share** one CR3 / mm.
3. **No shared-AS workarounds** (parent-image snapshot and alternate child-stack VAs are **gone**).
4. Every PCB holds a **`cr3`** (PML4 physical address). `0` means “not set yet — use kernel reference CR3.”
5. **`free_mm` only when the last task using that CR3 exits** (thread-group / shared-mm safe).

---

## Near-term virtual layout (identity kernel)

| Region | Virtual address | Notes |
|--------|-----------------|--------|
| Identity / low RAM | `0 … ~1 GiB` | Kernel text/data, early page tables, VGA, phys access via identity |
| Kernel load | `0x0010_0000` (1 MiB) | Matches Multiboot / `linker.ld` |
| Kernel heap | `0x0000_0001_0000_0000` | Demand-mapped; not identity |
| ELF load scratch | `0x0000_0001_3000_0000` | Kernel buffer for reading ELF from disk |
| User ET_EXEC (typical) | `0x0040_0000` … | Static BusyBox/musl; private frames per mm |
| User stack (classic) | top ~`0x0000_0000_7FFF_F000` | 1 MiB window today |
| User mmap arena | process `mmap_bump` | Anon + file-backed `MAP_PRIVATE` snapshot |
| Signal restorer | `0x7ffd0000` | Kernel trampoline page (`rt_sigreturn`) |

Canonical user half ends at `0x0000_8000_0000_0000` (non-canonical gap).

---

## Phase checklist (mm-related + process foundation)

| Slice | Goal | Status |
|-------|------|--------|
| **1a** | `Process.cr3`, `kernel_cr3()`, `switch_mm()` | done |
| **1b** | `clone_mm` + private CR3 on fork | done |
| **1c** | Same-VA private stack copy; copy USER leaves | done |
| **1d** | Drop parent-image snapshot helpers | done |
| **3a** | Cooperative `sched` + timer `need_resched` | done |
| **3b** | `fork` → Ready only; `wait` schedules child; `wake_up`/`sleep_current` | done |
| **3c** | Timer IRQ user→user preemption via `TrapFrame` + `try_preempt` | done |
| **3d** | Per-process kernel stacks (TSS/syscall) + `preempt` counter | done |
| **3e** | Nest-safe IRQ preempt (`entered_via_nest`, `resume_user_trap`, TSS≠nest) | done |
| **4a** | `tid`/`tgid`, `gettid`, `getpid`=tgid | done |
| **4b** | `clone` (`CLONE_VM`/`THREAD`/settids + stack) | done |
| **4c** | Shared FD tables (refcount), `exit_group` | done |
| **5a** | kill/tkill/tgkill, masks, default terminate | done |
| **5b** | User handlers + restorer; Ctrl-C → SIGINT | done |
| **6a** | `futex` WAIT/WAKE + PRIVATE | done |
| **6b** | `clear_child_tid` store 0 + wake on exit | done |

### Preemption policy (Phase 3)

- IRQ on per-process TSS.RSP0 (**not** nest syscall stack)
- sticky `entered_via_nest` + `resume_user_trap` / `return_from_user`
- preempt at nest depth 0–1; depth ≥ 2 stays cooperative
- execve does not steal the launcher nest frame after IRQ-scheduled children

Kernel shell: `preempt` / `preempttest` (A–G). Userspace: `clonetest`, `signaltest`, `futextest`.

**Next (not mm):** Phase 9 file-map ELF / `MAP_SHARED` toward dynlink (P10). Optional mm polish: COW fork, higher-half, demand paging. P8 is closed; IDE remains built-in. Destination: [ROADMAP.md](ROADMAP.md) (install a Linux DE).

---

## Later (not now)

- Higher-half kernel (e.g. `-2 GiB` / `0xFFFF_FFFF_8000_0000`)
- COW fork
- Demand paging / stack growth on #PF
- `MAP_SHARED` / COW file mmap (P9b is private snapshot only)

See also: [ROADMAP.md](ROADMAP.md), [ABI.md](ABI.md).
