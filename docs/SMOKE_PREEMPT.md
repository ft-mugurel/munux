# Specific tests: IRQ preemption

## What is under test

| Mechanism | Code |
|-----------|------|
| Timer IRQ saves/restores `TrapFrame` | `sched::try_preempt` |
| Per-process kernel stack for IRQ (TSS RSP0) | `process/kstack.rs` |
| Nest stack ≠ IRQ stack | `push_syscall_stack` |
| Exit of IRQ-resumed task | `resume_user_trap` vs `return_from_user` |
| Counter of real switches | `irq_preempt_count` / `preempt_count()` |
| `need_resched` + nest gates | `try_preempt` early returns |

## Kernel command: `munux> preempttest`

Runs **seven focused checks** (A–G). Boot, exit userspace shell if needed, then:

```text
$ exit
munux> preempttest
```

### A — Synthetic A→B→A (core switch)

Creates two Ready/Running PCBs with fake user `TrapFrame`s, forces
`test_try_preempt` twice.

| Expect | Meaning |
|--------|---------|
| `switches=2` | count advanced |
| first next pid = B, `rip=0x400100` | frame rewritten to next task |
| second pid = A, `rip=0x400000` | round-trip restore |

**PASS** = full A→B and B→A. This proves `try_preempt` + trap rewrite without real IRQ.

### B — Kernel CS → no switch

Same two-task setup, but `frame.cs` is kernel code (ring 0).

| Expect | Meaning |
|--------|---------|
| `count_delta=0` | no preempt |
| pid unchanged | never switch out of kernel |

**PASS** = IRQ preemption only for user mode.

### C — No Ready peer → no switch

Only task A is Running; no other Ready.

| Expect | Meaning |
|--------|---------|
| `count_delta=0` | pick_ready found nothing |
| `still_pid=A`, frame rip unchanged | no false switch |

### D — Trap save integrity

Before forced switch, fill frame with markers (`rax=0xA11…`, `rbx`, `rip=0x401234`, …).
After A→B:

| Expect | Meaning |
|--------|---------|
| current = B, `frame.rip=0x400100` | next loaded |
| A.trap holds all markers | interrupted context saved on prev PCB |
| A.state = Ready | prev is schedulable again |

### E — Kstack slots + TSS install

| Expect | Meaning |
|--------|---------|
| `top[0]`, `top[1]`, `top[2]` all distinct | per-process stacks |
| `install_for_slot(n)` → TSS.RSP0 == `top[n]` | IRQ lands on right stack |

### F — `need_resched` gate

Ready peer present, but `clear_need_resched()` and call `try_preempt` **without** force.

| Expect | Meaning |
|--------|---------|
| `count_delta=0` | timer flag required |
| pid / rip unchanged | cooperative gate works |

### G — Userspace dual-spin (embedded `preempttest`)

Real fork + dual 80M spin + wait at **nest depth 1** (kernel `run` /
`preempttest`). IRQ preempt is enabled at depth 0–1 → expect **PASS**
with `irq_switches >= 1`. (Under shell wait, depth ≥ 2 stays cooperative.)

| Result | Meaning |
|--------|---------|
| `irq_switches>=1` | **PASS** — Phase 3 exit criterion |
| `irq_switches==0` + clean exit | **WEAK** — QEMU timing (re-run) |
| panic / FAIL run | **FAIL** |

## Other commands

```text
munux> preempt          # irq_preempt_count + need_resched
$ preempttest           # userspace binary only (from /bin or embed)
$ forktest              # regression
$ busybox true
```

## Pass criteria (overall)

| Check | Pass |
|-------|------|
| A–F all PASS | required for unit path |
| G PASS or WEAK | required (no panic) |
| `fail=0` in summary | required |
| Rest of smoke green | required |

## Last QEMU result (2026-07-31)

```text
forktest / busybox true / busybox uname / shell exit — OK
A–G: PASS (G irq_switches=145)
preempttest: overall OK (fails=0)
run sh → forktest → busybox true → exit — OK
```

## Why dual spin (G)

```text
fork
  parent: spin 80M  ─┐
  child:  spin 80M   ─┴ both Ready/Running while parent not yet in wait
wait
```

Timer (100 Hz) can switch between them **if** nest IRQ preemption is enabled.
With nest gate on, expect **WEAK**. With nest-safe path fully on, expect **PASS**.
