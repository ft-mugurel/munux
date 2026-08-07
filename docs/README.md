# munux documentation index

| Document | Description |
|----------|-------------|
| **[../README.md](../README.md)** | Project overview, build, current capabilities |
| **[ROADMAP.md](ROADMAP.md)** | Architecture plan: mm → schedule → threads → modules |
| **[MM.md](MM.md)** | Memory layout + phase checklist (P1–P6 foundation; P7–P8 done, P9 next) |
| **[SMOKE_VFS.md](SMOKE_VFS.md)** | VFS mounts / fops / pipes (Phase 7 practical) |
| **[SMOKE_MODULE.md](SMOKE_MODULE.md)** | Modules: MNX1 + ELF `.ko`, hello + `/dev/echo` (P8a–8c) |
| **[ABI.md](ABI.md)** | Syscall calling convention, process/FD model (**v0.3.1**, 76 numbers) |
| **[SYSCALL_COMPARE.md](SYSCALL_COMPARE.md)** | Full Linux x86_64 vs munux syscall matrix |
| **[SMOKE_PREEMPT.md](SMOKE_PREEMPT.md)** | IRQ preemption (`preempttest` A–G) |
| **[SMOKE_CLONE.md](SMOKE_CLONE.md)** | `clone` / tid smoke |
| **[SMOKE_SIGNAL.md](SMOKE_SIGNAL.md)** | Signals + Ctrl-C |
| **[SMOKE_FUTEX.md](SMOKE_FUTEX.md)** | Futex join smoke |
| **[BUSYBOX_SUITE_REPORT.md](BUSYBOX_SUITE_REPORT.md)** | Strict BusyBox regression suite (primary probe report) |
| **[BUSYBOX_SUITE_RESULTS.json](BUSYBOX_SUITE_RESULTS.json)** | Machine-readable suite results |
| **[BUSYBOX_REPORT.md](BUSYBOX_REPORT.md)** | **Superseded** zero-arg applet scan (historical only) |
| **[../SMOKE.md](../SMOKE.md)** | Manual smoke checklist |

## How to use these docs

1. **New contributor** → README → ROADMAP → ABI  
2. **Implementing a syscall** → ABI + SYSCALL_COMPARE  
3. **Regression after a change** → SMOKE + focused SMOKE_* + BusyBox suite  
4. **Planning next architecture** → ROADMAP (P8 closed; Phase 9 next; IDE stays built-in)
