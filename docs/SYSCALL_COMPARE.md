# Linux x86_64 syscalls vs munux

**Last updated:** 2026-08-07 (count synced to dispatch `match`; qemu-connect smoke).  
Source of Linux names/numbers: host `/usr/include/asm/unistd_64.h`.  
Source of munux set: `src/syscalls/mod.rs` dispatch `match` (not merely `num` constants — `READLINK` is defined but ENOSYS).

**Product goal:** Linux-compatible kernel (see [ROADMAP.md](ROADMAP.md)) — not 100% syscall count for its own sake.

| Metric | Value |
|--------|------:|
| Linux (unistd_64.h) | **385** |
| munux dispatched | **81** |
| Coverage | **21.0%** |
| Notable ENOSYS | `poll`/`epoll`/`vfork`/`prctl`/sockets/… |

Legend in the tables below: implemented rows list munux notes; “NOT in munux” means **`-ENOSYS`**.

## munux implemented (by number)

| # | Linux name | munux const | Notes |
|--:|------------|-------------|-------|
| 0 | `read` | READ | implemented |
| 1 | `write` | WRITE | implemented |
| 2 | `open` | OPEN | implemented |
| 3 | `close` | CLOSE | implemented |
| 4 | `stat` | STAT | implemented |
| 5 | `fstat` | FSTAT | implemented |
| 6 | `lstat` | LSTAT | implemented |
| 8 | `lseek` | LSEEK | implemented |
| 9 | `mmap` | MMAP | partial (`MAP_PRIVATE` anon + file copy-in; r9 offset; no `MAP_SHARED`) |
| 10 | `mprotect` | MPROTECT | implemented |
| 11 | `munmap` | MUNMAP | implemented |
| 12 | `brk` | BRK | implemented |
| 13 | `rt_sigaction` | RT_SIGACTION | done (handler / IGN / DFL) |
| 14 | `rt_sigprocmask` | RT_SIGPROCMASK | done (64-bit mask) |
| 15 | `rt_sigreturn` | RT_SIGRETURN | done (restorer) |
| 16 | `ioctl` | IOCTL | partial (TTY probes) |
| 19 | `readv` | READV | implemented |
| 20 | `writev` | WRITEV | implemented |
| 21 | `access` | ACCESS | implemented |
| 22 | `pipe` | PIPE | done (P7d; cooperative) |
| 32 | `dup` | DUP | done |
| 33 | `dup2` | DUP2 | done |
| 35 | `nanosleep` | NANOSLEEP | implemented |
| 39 | `getpid` | GETPID | implemented (returns **tgid**) |
| 40 | `sendfile` | SENDFILE | partial |
| 56 | `clone` | CLONE | done (VM/FILES/THREAD/settids/TLS/stack) |
| 57 | `fork` | FORK | done (private CR3 `clone_mm`) |
| 59 | `execve` | EXECVE | implemented |
| 60 | `exit` | EXIT | implemented (+ clear_child_tid wake) |
| 61 | `wait4` | WAIT4 | implemented |
| 62 | `kill` | KILL | done (process-directed) |
| 63 | `uname` | UNAME | implemented |
| 72 | `fcntl` | FCNTL | partial |
| 79 | `getcwd` | GETCWD | implemented |
| 80 | `chdir` | CHDIR | implemented |
| 82 | `rename` | RENAME | done (vops) |
| 83 | `mkdir` | MKDIR | implemented |
| 84 | `rmdir` | RMDIR | implemented |
| 86 | `link` | LINK | done (vops; hard link; last symlink not followed) |
| 87 | `unlink` | UNLINK | implemented |
| 88 | `symlink` | SYMLINK | done (P9a; ext2 fast + block) |
| 89 | `readlink` | READLINK | done |
| 90 | `chmod` | CHMOD | implemented |
| 92 | `chown` | CHOWN | stub (always success) |
| 95 | `umask` | UMASK | implemented |
| 96 | `gettimeofday` | GETTIMEOFDAY | implemented |
| 102 | `getuid` | GETUID | implemented |
| 104 | `getgid` | GETGID | implemented (0) |
| 105 | `setuid` | SETUID | stub |
| 106 | `setgid` | SETGID | stub |
| 107 | `geteuid` | GETEUID | implemented |
| 108 | `getegid` | GETEGID | implemented (0) |
| 110 | `getppid` | GETPPID | implemented |
| 115 | `getgroups` | GETGROUPS | implemented |
| 158 | `arch_prctl` | ARCH_PRCTL | implemented |
| 162 | `sync` | SYNC | stub (no-op) |
| 175 | `init_module` | INIT_MODULE | done (MNX1 image; params ignored) |
| 176 | `delete_module` | DELETE_MODULE | done (EBUSY if refcount > 0) |
| 186 | `gettid` | GETTID | done (unique tid) |
| 200 | `tkill` | TKILL | done |
| 202 | `futex` | FUTEX | done (WAIT/WAKE/REQUEUE/CMP_REQUEUE + PRIVATE + relative timeout) |
| 217 | `getdents64` | GETDENTS64 | implemented |
| 218 | `set_tid_address` | SET_TID_ADDRESS | done (+ clear wake on exit) |
| 228 | `clock_gettime` | CLOCK_GETTIME | implemented |
| 231 | `exit_group` | EXIT_GROUP | done (thread group) |
| 234 | `tgkill` | TGKILL | done |
| 235 | `utimes` | UTIMES | implemented |
| 257 | `openat` | OPENAT | partial (AT_FDCWD/abs) |
| 258 | `mkdirat` | MKDIRAT | partial |
| 261 | `futimesat` | FUTIMESAT | partial |
| 262 | `newfstatat` | NEWFSTATAT | implemented |
| 263 | `unlinkat` | UNLINKAT | partial |
| 264 | `renameat` | RENAMEAT | partial |
| 266 | `symlinkat` | SYMLINKAT | partial (AT_FDCWD / abs) |
| 267 | `readlinkat` | READLINKAT | partial (AT_FDCWD / abs) |
| 268 | `fchmodat` | FCHMODAT | partial |
| 269 | `faccessat` | FACCESSAT | partial |
| 280 | `utimensat` | UTIMENSAT | partial |
| 293 | `pipe2` | PIPE2 | done (flags ignored) |
| 313 | `finit_module` | FINIT_MODULE | done (load from fd; MNX1 or ELF ET_REL) |
| 332 | `statx` | STATX | done (basic mask; AT_SYMLINK_NOFOLLOW) |

## Linux syscalls NOT in munux (ENOSYS)

Among many others, these were previously over-claimed as implemented in older docs:

| # | name | note |
|--:|------|------|
| 7 | `poll` | not dispatched |
| 58 | `vfork` | not dispatched (use `fork`) |
| 99 | `sysinfo` | not dispatched |
| 109 | `setpgid` | not dispatched |
| 111 | `getpgrp` | not dispatched |
| 112 | `setsid` | not dispatched |
| 121 | `getpgid` | not dispatched |
| 137 | `statfs` | not dispatched |
| 138 | `fstatfs` | not dispatched |
| 140 | `getpriority` | not dispatched |
| 141 | `setpriority` | not dispatched |
| 271 | `ppoll` | not dispatched |


Full remaining list (alphabetical by number):


| # | name |
|--:|------|
| 17 | `pread64` |
| 18 | `pwrite64` |
| 23 | `select` |
| 24 | `sched_yield` |
| 25 | `mremap` |
| 26 | `msync` |
| 27 | `mincore` |
| 28 | `madvise` |
| 29 | `shmget` |
| 30 | `shmat` |
| 31 | `shmctl` |
| 34 | `pause` |
| 36 | `getitimer` |
| 37 | `alarm` |
| 38 | `setitimer` |
| 41 | `socket` |
| 42 | `connect` |
| 43 | `accept` |
| 44 | `sendto` |
| 45 | `recvfrom` |
| 46 | `sendmsg` |
| 47 | `recvmsg` |
| 48 | `shutdown` |
| 49 | `bind` |
| 50 | `listen` |
| 51 | `getsockname` |
| 52 | `getpeername` |
| 53 | `socketpair` |
| 54 | `setsockopt` |
| 55 | `getsockopt` |
| 64 | `semget` |
| 65 | `semop` |
| 66 | `semctl` |
| 67 | `shmdt` |
| 68 | `msgget` |
| 69 | `msgsnd` |
| 70 | `msgrcv` |
| 71 | `msgctl` |
| 73 | `flock` |
| 74 | `fsync` |
| 75 | `fdatasync` |
| 76 | `truncate` |
| 77 | `ftruncate` |
| 78 | `getdents` |
| 81 | `fchdir` |
| 85 | `creat` |
| 91 | `fchmod` |
| 93 | `fchown` |
| 94 | `lchown` |
| 97 | `getrlimit` |
| 98 | `getrusage` |
| 100 | `times` |
| 101 | `ptrace` |
| 103 | `syslog` |
| 113 | `setreuid` |
| 114 | `setregid` |
| 116 | `setgroups` |
| 117 | `setresuid` |
| 118 | `getresuid` |
| 119 | `setresgid` |
| 120 | `getresgid` |
| 122 | `setfsuid` |
| 123 | `setfsgid` |
| 124 | `getsid` |
| 125 | `capget` |
| 126 | `capset` |
| 127 | `rt_sigpending` |
| 128 | `rt_sigtimedwait` |
| 129 | `rt_sigqueueinfo` |
| 130 | `rt_sigsuspend` |
| 131 | `sigaltstack` |
| 132 | `utime` |
| 133 | `mknod` |
| 134 | `uselib` |
| 135 | `personality` |
| 136 | `ustat` |
| 139 | `sysfs` |
| 142 | `sched_setparam` |
| 143 | `sched_getparam` |
| 144 | `sched_setscheduler` |
| 145 | `sched_getscheduler` |
| 146 | `sched_get_priority_max` |
| 147 | `sched_get_priority_min` |
| 148 | `sched_rr_get_interval` |
| 149 | `mlock` |
| 150 | `munlock` |
| 151 | `mlockall` |
| 152 | `munlockall` |
| 153 | `vhangup` |
| 154 | `modify_ldt` |
| 155 | `pivot_root` |
| 156 | `_sysctl` |
| 157 | `prctl` |
| 159 | `adjtimex` |
| 160 | `setrlimit` |
| 161 | `chroot` |
| 163 | `acct` |
| 164 | `settimeofday` |
| 165 | `mount` |
| 166 | `umount2` |
| 167 | `swapon` |
| 168 | `swapoff` |
| 169 | `reboot` |
| 170 | `sethostname` |
| 171 | `setdomainname` |
| 172 | `iopl` |
| 173 | `ioperm` |
| 174 | `create_module` |
| 177 | `get_kernel_syms` |
| 178 | `query_module` |
| 179 | `quotactl` |
| 180 | `nfsservctl` |
| 181 | `getpmsg` |
| 182 | `putpmsg` |
| 183 | `afs_syscall` |
| 184 | `tuxcall` |
| 185 | `security` |
| 187 | `readahead` |
| 188 | `setxattr` |
| 189 | `lsetxattr` |
| 190 | `fsetxattr` |
| 191 | `getxattr` |
| 192 | `lgetxattr` |
| 193 | `fgetxattr` |
| 194 | `listxattr` |
| 195 | `llistxattr` |
| 196 | `flistxattr` |
| 197 | `removexattr` |
| 198 | `lremovexattr` |
| 199 | `fremovexattr` |
| 201 | `time` |
| 203 | `sched_setaffinity` |
| 204 | `sched_getaffinity` |
| 205 | `set_thread_area` |
| 206 | `io_setup` |
| 207 | `io_destroy` |
| 208 | `io_getevents` |
| 209 | `io_submit` |
| 210 | `io_cancel` |
| 211 | `get_thread_area` |
| 212 | `lookup_dcookie` |
| 213 | `epoll_create` |
| 214 | `epoll_ctl_old` |
| 215 | `epoll_wait_old` |
| 216 | `remap_file_pages` |
| 219 | `restart_syscall` |
| 220 | `semtimedop` |
| 221 | `fadvise64` |
| 222 | `timer_create` |
| 223 | `timer_settime` |
| 224 | `timer_gettime` |
| 225 | `timer_getoverrun` |
| 226 | `timer_delete` |
| 227 | `clock_settime` |
| 229 | `clock_getres` |
| 230 | `clock_nanosleep` |
| 232 | `epoll_wait` |
| 233 | `epoll_ctl` |
| 236 | `vserver` |
| 237 | `mbind` |
| 238 | `set_mempolicy` |
| 239 | `get_mempolicy` |
| 240 | `mq_open` |
| 241 | `mq_unlink` |
| 242 | `mq_timedsend` |
| 243 | `mq_timedreceive` |
| 244 | `mq_notify` |
| 245 | `mq_getsetattr` |
| 246 | `kexec_load` |
| 247 | `waitid` |
| 248 | `add_key` |
| 249 | `request_key` |
| 250 | `keyctl` |
| 251 | `ioprio_set` |
| 252 | `ioprio_get` |
| 253 | `inotify_init` |
| 254 | `inotify_add_watch` |
| 255 | `inotify_rm_watch` |
| 256 | `migrate_pages` |
| 259 | `mknodat` |
| 260 | `fchownat` |
| 265 | `linkat` |
| 270 | `pselect6` |
| 272 | `unshare` |
| 273 | `set_robust_list` |
| 274 | `get_robust_list` |
| 275 | `splice` |
| 276 | `tee` |
| 277 | `sync_file_range` |
| 278 | `vmsplice` |
| 279 | `move_pages` |
| 281 | `epoll_pwait` |
| 282 | `signalfd` |
| 283 | `timerfd_create` |
| 284 | `eventfd` |
| 285 | `fallocate` |
| 286 | `timerfd_settime` |
| 287 | `timerfd_gettime` |
| 288 | `accept4` |
| 289 | `signalfd4` |
| 290 | `eventfd2` |
| 291 | `epoll_create1` |
| 292 | `dup3` |
| 294 | `inotify_init1` |
| 295 | `preadv` |
| 296 | `pwritev` |
| 297 | `rt_tgsigqueueinfo` |
| 298 | `perf_event_open` |
| 299 | `recvmmsg` |
| 300 | `fanotify_init` |
| 301 | `fanotify_mark` |
| 302 | `prlimit64` |
| 303 | `name_to_handle_at` |
| 304 | `open_by_handle_at` |
| 305 | `clock_adjtime` |
| 306 | `syncfs` |
| 307 | `sendmmsg` |
| 308 | `setns` |
| 309 | `getcpu` |
| 310 | `process_vm_readv` |
| 311 | `process_vm_writev` |
| 312 | `kcmp` |
| 314 | `sched_setattr` |
| 315 | `sched_getattr` |
| 316 | `renameat2` |
| 317 | `seccomp` |
| 318 | `getrandom` |
| 319 | `memfd_create` |
| 320 | `kexec_file_load` |
| 321 | `bpf` |
| 322 | `execveat` |
| 323 | `userfaultfd` |
| 324 | `membarrier` |
| 325 | `mlock2` |
| 326 | `copy_file_range` |
| 327 | `preadv2` |
| 328 | `pwritev2` |
| 329 | `pkey_mprotect` |
| 330 | `pkey_alloc` |
| 331 | `pkey_free` |
| 333 | `io_pgetevents` |
| 334 | `rseq` |
| 335 | `uretprobe` |
| 336 | `uprobe` |
| 424 | `pidfd_send_signal` |
| 425 | `io_uring_setup` |
| 426 | `io_uring_enter` |
| 427 | `io_uring_register` |
| 428 | `open_tree` |
| 429 | `move_mount` |
| 430 | `fsopen` |
| 431 | `fsconfig` |
| 432 | `fsmount` |
| 433 | `fspick` |
| 434 | `pidfd_open` |
| 435 | `clone3` |
| 436 | `close_range` |
| 437 | `openat2` |
| 438 | `pidfd_getfd` |
| 439 | `faccessat2` |
| 440 | `process_madvise` |
| 441 | `epoll_pwait2` |
| 442 | `mount_setattr` |
| 443 | `quotactl_fd` |
| 444 | `landlock_create_ruleset` |
| 445 | `landlock_add_rule` |
| 446 | `landlock_restrict_self` |
| 447 | `memfd_secret` |
| 448 | `process_mrelease` |
| 449 | `futex_waitv` |
| 450 | `set_mempolicy_home_node` |
| 451 | `cachestat` |
| 452 | `fchmodat2` |
| 453 | `map_shadow_stack` |
| 454 | `futex_wake` |
| 455 | `futex_wait` |
| 456 | `futex_requeue` |
| 457 | `statmount` |
| 458 | `listmount` |
| 459 | `lsm_get_self_attr` |
| 460 | `lsm_set_self_attr` |
| 461 | `lsm_list_modules` |
| 462 | `mseal` |
| 463 | `setxattrat` |
| 464 | `getxattrat` |
| 465 | `listxattrat` |
| 466 | `removexattrat` |
| 467 | `open_tree_attr` |
| 468 | `file_getattr` |
| 469 | `file_setattr` |
| 470 | `listns` |
| 471 | `rseq_slice_yield` |

Total missing: **304** (385 − 81). The numbered list below is the Linux x86_64 set minus munux dispatch; unused holes in `unistd_64.h` are omitted.
