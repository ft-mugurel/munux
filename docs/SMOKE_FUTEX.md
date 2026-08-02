# Phase 6 smoke: futex + clear_child_tid

## Implemented

| Piece | Notes |
|-------|--------|
| `futex` (202) | `FUTEX_WAIT` / `FUTEX_WAKE` (+ `PRIVATE`) |
| Wait queue | Keyed by user VA (+ CR3 for PRIVATE) |
| `clear_child_tid` | On exit / exit_group siblings: store 0 + wake |
| Cooperative wait | While waiting, `take_ready` runs other Ready tasks |

No timeout / requeue / PI yet.

## Test

```text
$ futextest
futextest: child ok
futextest: parent ok
$ clonetest
$ forktest
```

Also: `munux> run futextest`
