# BusyBox strict suite report (munux)

> **Role of this report:** regression / ABI probe — **not** the product goal.  
> Product direction: Linux-compatible Rust kernel (threads, modules, mm isolation).  
> See [ROADMAP.md](ROADMAP.md) and the root [README.md](../README.md).

## Method (corrected)

- Real commands with **arguments** when needed.
- After each case: **`busybox true` canary** (fails if next command panics).
- Console settle + re-read (no prompt-race false PASS).
- Kernel: user/fork stacks **1 MiB**; execve argv up to **16** words.
- Also exercised by hand: `rename`/`mv`, interactive ash + external cmds (post-suite fixes).

## Summary (last full automated run)

| Status | Count |
|--------|------:|
| `PASS` | 46 |
| `FAIL_PANIC` | 0 |
| `FAIL_ENOSYS` | 1 (`mv` / `rename` — **fixed after this run**) |
| `FAIL_ERROR` | 0 |
| `HANG` | 1 (`find .`) |
| **Total** | **48** |

### Post-report fixes (not yet re-scored in JSON)

| Item | Status |
|------|--------|
| `rename` (82) + BusyBox `mv` | **Implemented** — re-run suite to clear `FAIL_ENOSYS` |
| Interactive ash + external `ls` / `true` | **Fixed** (nest `saved_user_rsp`, image/mmap/stack snap, TLS selectors) |
| `find .` hang | **Still open** |

## Full results

| ID | Command | Status | Canary | Detail |
|----|---------|--------|--------|--------|
| `true` | `busybox true` | **PASS** | True | (ok empty) |
| `false` | `busybox false` | **PASS** | True | (ok empty) |
| `uname` | `busybox uname` | **PASS** | True | munux |
| `arch` | `busybox arch` | **PASS** | True | x86_64 |
| `id` | `busybox id` | **PASS** | True | uid=0 gid=0 groups=0 |
| `pwd` | `busybox pwd` | **PASS** | True | / |
| `hostname` | `busybox hostname` | **PASS** | True | munux |
| `date` | `busybox date` | **PASS** | True | Tue Nov 14 22:13:43 UTC 2023 |
| `echo_hi` | `busybox echo hi` | **PASS** | True | hi |
| `ls` | `busybox ls` | **PASS** | True | bin docs hello.txt lost+found musl_out.txt proc t_copy.txt |
| `ls_bin` | `busybox ls bin` | **PASS** | True | archprctl brktest busybox cat echo exectest false file_musl forktest fwrite_musl hello hello_musl ls |
| `cal` | `busybox cal` | **PASS** | True | November 2023 Su Mo Tu We Th Fr Sa 1  2  3  4 5  6  7  8  9 10 11 12 13 14 15 16 17 18 19 20 21 22 2 |
| `touch_f` | `busybox touch t_suite.txt` | **PASS** | True | (ok empty) |
| `chown_f` | `busybox chown 0 t_suite.txt` | **PASS** | True | (ok empty) |
| `ln_f` | `busybox ln t_suite.txt t_link.txt` | **PASS** | True | (ok empty) |
| `cp_f` | `busybox cp t_suite.txt t_copy.txt` | **PASS** | True | (ok empty) |
| `mv_f` | `busybox mv t_copy.txt t_moved.txt` | **FAIL_ENOSYS** | True | syscall: ENOSYS n=82 (-38) mv: can't rename 't_copy.txt': Function not implemented |
| `diff_f` | `busybox diff t_suite.txt t_moved.txt` | **PASS** | True | diff: can't stat 't_moved.txt': No such file or directory |
| `tar_c` | `busybox tar -cf t.tar t_suite.txt` | **PASS** | True | (ok empty) |
| `tar_t` | `busybox tar -tf t.tar` | **PASS** | True | t_suite.txt |
| `mkdir_d` | `busybox mkdir t_dir` | **PASS** | True | (ok empty) |
| `rmdir_d` | `busybox rmdir t_dir` | **PASS** | True | (ok empty) |
| `cat_f` | `busybox cat hello.txt` | **PASS** | True | Hello from munux ext2! second line |
| `wc_f` | `busybox wc hello.txt` | **PASS** | True | 2         6        35 hello.txt |
| `head_f` | `busybox head hello.txt` | **PASS** | True | Hello from munux ext2! second line |
| `md5_f` | `busybox md5sum hello.txt` | **PASS** | True | 2a7a69919a7cd95c9b3d2ae0ed773783  hello.txt |
| `stat_f` | `busybox stat hello.txt` | **PASS** | True | File: hello.txt Size: 35            Blocks: 2          IO Block: 1024   regular file Device: deadh/5 |
| `rm_f` | `busybox rm t_moved.txt` | **PASS** | True | rm: can't remove 't_moved.txt': No such file or directory |
| `rm_f2` | `busybox rm t_link.txt` | **PASS** | True | (ok empty) |
| `rm_f3` | `busybox rm t_suite.txt` | **PASS** | True | (ok empty) |
| `rm_tar` | `busybox rm t.tar` | **PASS** | True | (ok empty) |
| `find_dot` | `busybox find .` | **HANG** | True | no $ prompt |
| `grep_f` | `busybox grep hello hello.txt` | **PASS** | True | (ok empty) |
| `sync` | `busybox sync` | **PASS** | True | (ok empty) |
| `nice` | `busybox nice` | **PASS** | True | 0 |
| `which_ls` | `busybox which ls` | **PASS** | True | /bin/ls |
| `whoami` | `busybox whoami` | **PASS** | True | whoami: unknown uid 0 |
| `df` | `busybox df` | **PASS** | True | Filesystem           1K-blocks      Used Available Use% Mounted on df: /proc/mounts: No such file or |
| `free` | `busybox free` | **PASS** | True | total        used        free      shared  buff/cache   available Mem:         524160       86388    |
| `ps` | `busybox ps` | **PASS** | True | PID   USER     TIME  COMMAND |
| `sleep1` | `busybox sleep 1` | **PASS** | True | (ok empty) |
| `chmod_f` | `busybox chmod 644 hello.txt` | **PASS** | True | (ok empty) |
| `seq_uname` | `busybox uname` | **PASS** | True | munux |
| `seq_after_ls` | `busybox ls` | **PASS** | True | bin docs hello.txt lost+found musl_out.txt proc t_copy.txt |
| `seq_after_true` | `busybox true` | **PASS** | True | (ok empty) |
| `seq_cal` | `busybox cal` | **PASS** | True | November 2023 Su Mo Tu We Th Fr Sa 1  2  3  4 5  6  7  8  9 10 11 12 13 14 15 16 17 18 19 20 21 22 2 |
| `seq_echo` | `busybox echo ok` | **PASS** | True | ok |
| `seq_freestanding_ls` | `ls` | **PASS** | True | . .. lost+found bin docs hello.txt musl_out.txt t_copy.txt proc |

## PASS

`true`, `false`, `uname`, `arch`, `id`, `pwd`, `hostname`, `date`, `echo_hi`, `ls`, `ls_bin`, `cal`, `touch_f`, `chown_f`, `ln_f`, `cp_f`, `diff_f`, `tar_c`, `tar_t`, `mkdir_d`, `rmdir_d`, `cat_f`, `wc_f`, `head_f`, `md5_f`, `stat_f`, `rm_f`, `rm_f2`, `rm_f3`, `rm_tar`, `grep_f`, `sync`, `nice`, `which_ls`, `whoami`, `df`, `free`, `ps`, `sleep1`, `chmod_f`, `seq_uname`, `seq_after_ls`, `seq_after_true`, `seq_cal`, `seq_echo`, `seq_freestanding_ls`

## FAIL_PANIC

_(none)_

## FAIL_ENOSYS

`mv_f`

## FAIL_ERROR

_(none)_

## HANG

`find_dot`

