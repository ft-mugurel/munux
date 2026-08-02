# Phase 6 smoke: futex + clear_child_tid

## Implemented (6a–6c)

| Piece | Notes |
|-------|--------|
| `futex` (202) | `WAIT` / `WAKE` / `REQUEUE` / `CMP_REQUEUE` (+ `PRIVATE`) |
| Bitset ops | `WAIT_BITSET` / `WAKE_BITSET` (bitset ignored if non-zero) |
| Relative timeout | `struct timespec` on `WAIT` → `-ETIMEDOUT` (110) |
| Wait queue | Keyed by user VA (+ CR3 for PRIVATE) |
| `clear_child_tid` | On exit: store 0 + wake; non-leader threads auto-reaped |
| Cooperative wait | Runs **Ready children only** (not arbitrary tasks) |
| Nested wait | Nest depth ≥ 2 + no Ready child → spurious return (avoids deadlock) |

6th syscall arg (`val3` for CMP_REQUEUE / bitset) saved as `last_user_r9` in `syscall_entry`.

No PI / robust lists / absolute `CLOCK_REALTIME` wait yet.

## Test

```text
$ futextest
futextest: child ok
futextest: parent ok
$ clonetest
$ signaltest
$ forktest
```

Also: `munux> run futextest`

`futextest` covers: relative timeout, join via `clear_child_tid`, mutex unlock/wake, requeue path.

**Note:** `make disk` after changing `userland/futextest.asm` — exec prefers disk `/bin/futextest` over the embedded ELF.

## Design notes

- Under freestanding `sh`, wait is nest depth ≥ 2 (IRQ preempt off). Parent can only run a child to completion via cooperative `take_ready`. Mutex/requeue smokes unlock/requeue **before** join-runs the child.
- Do not call `take_ready(-1)` from futex wait — picking shell/kinit nest-corrupts and kills the waiter.
