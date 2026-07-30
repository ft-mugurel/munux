# munux documentation index

| Document | Description |
|----------|-------------|
| **[../README.md](../README.md)** | Project overview, build, current capabilities |
| **[ROADMAP.md](ROADMAP.md)** | Architecture plan: mm → schedule → threads → modules |
| **[ABI.md](ABI.md)** | Syscall calling convention, process/FD model (v0.3) |
| **[SYSCALL_COMPARE.md](SYSCALL_COMPARE.md)** | Full Linux x86_64 vs munux syscall matrix |
| **[BUSYBOX_SUITE_REPORT.md](BUSYBOX_SUITE_REPORT.md)** | Strict BusyBox regression suite (primary probe report) |
| **[BUSYBOX_SUITE_RESULTS.json](BUSYBOX_SUITE_RESULTS.json)** | Machine-readable suite results |
| **[BUSYBOX_REPORT.md](BUSYBOX_REPORT.md)** | **Superseded** zero-arg applet scan (historical only) |
| **[../SMOKE.md](../SMOKE.md)** | Manual smoke checklist |

## How to use these docs

1. **New contributor** → README → ROADMAP → ABI  
2. **Implementing a syscall** → ABI + SYSCALL_COMPARE  
3. **Regression after a change** → SMOKE + BusyBox suite  
4. **Planning threads/modules** → ROADMAP only (not applet lists)
