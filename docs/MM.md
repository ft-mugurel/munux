# munux memory layout (Phase 1 foundation)

**Status:** living layout freeze for the private-mm work.  
**Last updated:** 2026-07-31.

Higher-half is **not** required for Phase 1. The kernel stays in the **identity window** until a later dedicated milestone.

---

## Rules

1. **Kernel mapping** is the same in every process address space (shared page-table leaves or shared PML4 entries for the kernel window).
2. **User pages** must become **private frames** (not “load into identity VA==PA and hope”). Identity ET_EXEC is a **legacy workaround** to remove in Phase 1.
3. **No new shared-AS workarounds** (do not grow `USER_IMAGE_SNAP`, nest stacks, or alternate child-stack VAs). Fix isolation instead.
4. Every PCB holds a **`cr3`** (PML4 physical address). `0` means “not set yet — use kernel reference CR3.”

---

## Near-term virtual layout (identity kernel)

| Region | Virtual address | Notes |
|--------|-----------------|--------|
| Identity / low RAM | `0 … ~1 GiB` | Kernel text/data, early page tables, VGA, phys access via identity |
| Kernel load | `0x0010_0000` (1 MiB) | Matches Multiboot / `linker.ld` |
| Kernel heap | `0x0000_0001_0000_0000` | Demand-mapped; not identity |
| ELF load scratch | `0x0000_0001_3000_0000` | Kernel buffer for reading ELF from disk |
| User ET_EXEC (typical) | `0x0040_0000` … | Static BusyBox/musl; **must become private frames** |
| User stack (classic) | top ~`0x0000_0000_7FFF_F000` | 1 MiB window today |
| User mmap arena | process `mmap_bump` | Anonymous `MAP_PRIVATE` only today |
| Child stack slots (legacy) | `0x1_6000_0000` + stride | Outside identity; fork workaround — **delete after private mm** |

Canonical user half ends at `0x0000_8000_0000_0000` (non-canonical gap).

---

## Phase 1 targets (in order)

| Slice | Goal | Status |
|-------|------|--------|
| **1a** | `Process.cr3`, `kernel_cr3()`, `switch_mm()` | done |
| **1b** | `clone_mm` + private CR3 on fork | done |
| **1c** | Same-VA private stack copy; copy USER leaves; skip image snap when CR3 differs | done |
| **1d** | Drop snapshot helpers entirely | done |
| **3a** | Cooperative `sched` + timer `need_resched` | done |
| **3b** | `fork` → Ready only; `wait` schedules child; `wake_up`/`sleep_current` | done |
| **3c** | Timer IRQ user→user preemption via `TrapFrame` + `try_preempt` | done |
| **3d** | Per-process kernel stacks (TSS/syscall) + `preempt` counter | done |
| **3e** | Nest-safe IRQ preempt (`entered_via_nest`, `resume_user_trap`, TSS≠nest) | done |

Phase 3 complete: timer user→user preempt with nest-safe policy:
- IRQ on per-process TSS.RSP0 (not nest syscall stack)
- sticky `entered_via_nest` + `resume_user_trap` / `return_from_user`
- preempt at nest depth 0–1 (top-level dual-spin); depth ≥ 2 stays cooperative
  (avoids IRQ vs wait/exec nest races)
- execve does not steal the launcher nest frame after IRQ-scheduled children

Kernel shell: `preempt` / `preempttest` (A–G). **Next:** Phase 4 `clone`.
| **1c** | ELF/stack on private frames (stop identity USER promote for loads) | |
| **1d** | Drop parent image snapshot | |
| **1e** | Drop child-stack VA hack if possible | |
| **1f** | free private user frames on exit (with 1b+) | |

---

## Later (not now)

- Higher-half kernel (e.g. `-2 GiB` / `0xFFFF_FFFF_8000_0000`)
- COW fork
- Demand paging / stack growth on #PF

See also: [ROADMAP.md](ROADMAP.md), [ABI.md](ABI.md).
