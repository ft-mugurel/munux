# ⚠️ SUPERSEDED (historical)

This report was based on a **flawed zero-arg harness** that:

1. Treated BusyBox **usage banners** as success (many applets print usage without doing real work).
2. Missed **kernel panics** after the next command (prompt race / no canary).
3. Did not pass real **file arguments**.

**Do not use these WORKS/CRASH counts for planning.**  
Many “CRASH” / “ENOSYS” rows are **obsolete** after later syscall and ash work.

| Use this | For |
|----------|-----|
| [`BUSYBOX_SUITE_REPORT.md`](BUSYBOX_SUITE_REPORT.md) | Strict ~48-case regression |
| `scripts/busybox_suite.py` | Automated runner |
| [`ROADMAP.md`](ROADMAP.md) | Kernel architecture goals |
| [`SYSCALL_COMPARE.md`](SYSCALL_COMPARE.md) | Linux vs munux syscall map |

---

# BusyBox applet report — munux (archived zero-arg scan)

## How this was tested

- **Binary:** BusyBox **v1.36.1** static (`build/rootfs/bin/busybox`)
- **Applets:** 303 from `busybox --list` (excluding `[` and `[[`)
- **Guest:** munux x86_64 (qemu-connect), freestanding `/bin/sh`
- **Invocation:** `busybox <applet>` only (no extra arguments)
- **Harness:** automated run of every applet; console inspected for `$` return, ENOSYS logs, panics, hangs

### What “WORKS” means

The applet **started under munux**, produced output or exited, and control returned to the `$` prompt. That includes:

- clean success (`true`, `ls`, `pwd`)
- usage/help when args missing (`mkdir`, `cp`, `sleep`)
- normal userspace errors (`kill` with no target, `expr` with no args)

It does **not** prove full behavior with files, pipes, or network.

### Test limitations

1. No file arguments (so `cat`/`wc`/`md5sum` hang on stdin — expected).
2. Freestanding sh cannot pass multi-word argv (`busybox echo hi` becomes one token).
3. Interactive tools (`vi`, `ash`, `top`) are expected to hang without a TTY script.
4. Some CRASH results may be aggravated by shared-AS exec of a 1 MiB binary in a tight loop; still treated as failures.

## Summary

| Result | Count | % | Meaning |
|--------|------:|--:|---------|
| **WORKS** | 132 | 44% | Ran and returned to shell |
| **HANG** | 56 | 18% | Blocked (stdin/interactive/network) — no prompt |
| **FAIL_ENOSYS** | 35 | 12% | Hit unimplemented syscall(s) |
| **CRASH** | 78 | 26% | Kernel panic / CPU exception |
| **UNCLEAR** | 2 | 1% | Could not classify reliably |
| **Total** | **303** | 100% | |

- Runnable smoke (**WORKS**): **132**
- Hang without args (**HANG**): **56**
- Missing syscalls (**FAIL_ENOSYS**): **35**
- Kernel crash (**CRASH**): **78**

## WORKS (132)

`addgroup`, `arch`, `awk`, `basename`, `blkdiscard`, `blockdev`, `brctl`, `cal`, `chattr`, `chmod`, `chown`, `chroot`, `chvt`, `cmp`, `cp`, `cpio`, `cut`, `date`, `delgroup`, `deluser`, `diff`, `dirname`, `du`, `echo`, `egrep`, `env`, `ether-wake`, `expr`, `fallocate`, `false`, `fatattr`, `fbsplash`, `fdflush`, `fgrep`, `find`, `findfs`, `fstrim`, `fsync`, `fuser`, `getopt`, `grep`, `hostid`, `hostname`, `id`, `init`, `inotifyd`, `insmod`, `ip`, `ipcalc`, `ipcrm`, `kill`, `killall`, `link`, `ln`, `login`, `losetup`, `ls`, `lzma`, `mesg`, `microcom`, `mkdir`, `mkfifo`, `mknod`, `mkswap`, `modinfo`, `mountpoint`, `mv`, `nanddump`, `nandwrite`, `nbd-client`, `nohup`, `nsenter`, `nslookup`, `partprobe`, `pgrep`, `pidof`, `ping`, `ping6`, `pivot_root`, `pkill`, `pmap`, `printenv`, `printf`, `pscan`, `pwd`, `pwdx`, `raidautorun`, `rdev`, `readahead`, `readlink`, `realpath`, `renice`, `reset`, `rfkill`, `rm`, `rmdir`, `rmmod`, `run-parts`, `sed`, `seq`, `setfont`, `setkeycodes`, `setpriv`, `setserial`, `setsid`, `showkey`, `shred`, `slattach`, `sleep`, `ssl_client`, `stty`, `swapoff`, `swapon`, `switch_root`, `tar`, `test`, `timeout`, `touch`, `tr`, `true`, `truncate`, `tty`, `ttysize`, `uname`, `unlink`, `unzip`, `usleep`, `vconfig`, `watch`, `watchdog`, `whois`, `zcip`

## HANG — no return to prompt (56)

Typically waiting for **stdin**, a TTY, or a peer. Not always a kernel bug.

`base64`, `bc`, `bunzip2`, `bzcat`, `bzip2`, `chpasswd`, `cksum`, `clear`, `cryptpw`, `dc`, `dos2unix`, `expand`, `factor`, `fold`, `gunzip`, `gzip`, `hd`, `head`, `hexdump`, `loadfont`, `logger`, `lzcat`, `lzopcat`, `makemime`, `md5sum`, `mkpasswd`, `nl`, `od`, `paste`, `reformime`, `rev`, `sha1sum`, `sha256sum`, `sha3sum`, `sha512sum`, `shuf`, `sort`, `split`, `strings`, `sum`, `tac`, `tail`, `tee`, `unexpand`, `uniq`, `unix2dos`, `unlzma`, `unlzop`, `unxz`, `uudecode`, `wc`, `xargs`, `xxd`, `xzcat`, `yes`, `zcat`

## FAIL_ENOSYS — unimplemented syscall (35)

`add-shell`, `adjtimex`, `arp`, `arping`, `ash`, `cat`, `dd`, `dmesg`, `halt`, `ionice`, `ipaddr`, `ipcs`, `ipneigh`, `iproute`, `iprule`, `less`, `linux32`, `linux64`, `logread`, `more`, `nc`, `nice`, `nproc`, `poweroff`, `reboot`, `remove-shell`, `resize`, `sendmail`, `sync`, `unshare`, `uptime`, `uuencode`, `vi`, `wget`, `who`

### ENOSYS numbers observed

| # | Approx. Linux name | Seen |
|--:|--------------------|-----:|
| 41 | `socket` | 7 |
| 13 | `rt_sigaction` | 6 |
| 40 | `sendfile` | 3 |
| 35 | `nanosleep` | 3 |
| 89 | `readlink` | 2 |
| 135 | `personality` | 2 |
| 159 | `adjtimex` | 1 |
| 103 | `syslog` | 1 |
| 252 | `ioprio_get` | 1 |
| 71 | `msgget` | 1 |
| 29 | `shmget` | 1 |
| 140 | `getpriority` | 1 |
| 204 | `sched_getaffinity` | 1 |
| 33 | `dup2` | 1 |
| 162 | `sync` | 1 |
| 272 | `unshare` | 1 |
| 99 | `sysinfo` | 1 |
| 95 | `umask` | 1 |
| 7 | `pread64?` | 1 |

**Most important gaps for BusyBox:** `socket`(41), `dup2`(33), `readlink`(89), `rt_sigaction`(13), `sendfile`(40), `nanosleep`(35), `sync`(162), `umask`(95), `sysinfo`(99).

## CRASH — kernel panic / exception (78)

`acpid`, `adduser`, `bbconfig`, `beep`, `blkid`, `chgrp`, `comm`, `crond`, `crontab`, `deallocvt`, `depmod`, `df`, `dnsdomainname`, `dumpkmap`, `eject`, `fbset`, `fdisk`, `flock`, `free`, `fsck`, `getty`, `groups`, `hwclock`, `ifconfig`, `ifdown`, `ifenslave`, `ifup`, `install`, `iostat`, `iplink`, `iptunnel`, `kbd_mode`, `killall5`, `klogd`, `last`, `loadkmap`, `lsattr`, `lsmod`, `lsof`, `lsusb`, `mdev`, `mkdosfs`, `mktemp`, `modprobe`, `mount`, `mpstat`, `nameif`, `netstat`, `nmeter`, `nologin`, `ntpd`, `openvt`, `passwd`, `pipe_progress`, `ps`, `pstree`, `rdate`, `route`, `setconsole`, `setlogcons`, `sh`, `stat`, `su`, `sysctl`, `syslogd`, `time`, `top`, `traceroute`, `traceroute6`, `tree`, `tunctl`, `udhcpc`, `udhcpc6`, `umount`, `vlock`, `volname`, `which`, `whoami`

## UNCLEAR (2)

`lzop`, `mkfs.vfat`

## Notable highlights

| Applet | Result | Notes |
|--------|--------|-------|
| `ls` | WORKS | Lists root (bin, docs, hello.txt, …) |
| `true` / `false` | WORKS | Exit status path |
| `echo` | WORKS | No-args empty line |
| `pwd` | WORKS | prints `/` |
| `uname` | WORKS | `munux` |
| `id` | WORKS | uid/gid 0 |
| `date` | WORKS | PIT-based wall clock |
| `find` | WORKS* | Worked in retest (lists tree) |
| `grep` | WORKS* | Usage/help path |
| `tar` | WORKS* | Usage options listed |
| `cat` / `wc` / `md5sum` | HANG or ENOSYS | stdin / sendfile |
| `mkdir` / `rm` / `cp` / … | WORKS (usage) in full pass | Need args for real FS ops |
| network (`arp`,`wget`,`nc`,…) | ENOSYS | need `socket` |
| `reboot`/`halt`/`poweroff` | ENOSYS | reboot/power syscalls |
| `vi` | HANG | interactive + some ENOSYS |
| `ash` / `sh` | HANG / ENOSYS | interactive shell |

## Full table

| Applet | Bucket | Fine status | Detail (truncated) |
|--------|--------|-------------|------------------|
| `acpid` | **CRASH** | `crash_panic` | panic |
| `addgroup` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `add-shell` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=89 (-38) |
| `adduser` | **CRASH** | `crash_panic` | panic |
| `adjtimex` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=159 (-38) |
| `arch` | **WORKS** | `works` | x86_64 |
| `arp` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=41 (-38) |
| `arping` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=41 (-38) |
| `ash` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=13 (-38) |
| `awk` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `base64` | **HANG** | `hang_likely` | no prompt after wait |
| `basename` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `bbconfig` | **CRASH** | `crash_panic` | panic |
| `bc` | **HANG** | `hang_timeout` | no prompt after wait |
| `beep` | **CRASH** | `crash_panic` | panic |
| `blkdiscard` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `blkid` | **CRASH** | `crash_panic` | panic |
| `blockdev` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `brctl` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `bunzip2` | **HANG** | `hang_likely` | no prompt after wait |
| `bzcat` | **HANG** | `hang_likely` | no prompt after wait |
| `bzip2` | **HANG** | `hang_timeout` | no prompt after wait |
| `cal` | **WORKS** | `works` | November 2023 |
| `cat` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=40 (-38) |
| `chattr` | **WORKS** | `works` | P    Hierarchical project ID dir |
| `chgrp` | **CRASH** | `crash_panic` | panic |
| `chmod` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `chown` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `chpasswd` | **HANG** | `hang_timeout` | no prompt after wait |
| `chroot` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `chvt` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `cksum` | **HANG** | `hang_timeout` | no prompt after wait |
| `clear` | **HANG** | `hang_timeout` | [H[J[J$ |
| `cmp` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `comm` | **CRASH** | `crash_panic` | panic |
| `cp` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `cpio` | **WORKS** | `works` | -u    Overwrite |
| `crond` | **CRASH** | `crash_panic` | panic |
| `crontab` | **CRASH** | `crash_panic` | panic |
| `cryptpw` | **HANG** | `hang_timeout` | no prompt after wait |
| `cut` | **WORKS** | `works` | cut: expected a list of bytes, characters, or fields |
| `date` | **WORKS** | `works` |  |
| `dc` | **HANG** | `hang_timeout` | no prompt after wait |
| `dd` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=13 (-38) |
| `deallocvt` | **CRASH** | `crash_panic` | panic |
| `delgroup` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `deluser` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `depmod` | **CRASH** | `crash_panic` | panic |
| `df` | **CRASH** | `crash_panic` | panic |
| `diff` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `dirname` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `dmesg` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=103 (-38) |
| `dnsdomainname` | **CRASH** | `crash_panic` | panic |
| `dos2unix` | **HANG** | `hang_timeout` | no prompt after wait |
| `du` | **WORKS** | `works` | 12    ./lost+found |
| `dumpkmap` | **CRASH** | `crash_panic` | panic |
| `echo` | **WORKS** | `works` |  |
| `egrep` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `eject` | **CRASH** | `crash_panic` | panic |
| `env` | **WORKS** | `works` | (no output, clean exit) |
| `ether-wake` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `expand` | **HANG** | `hang_timeout` | no prompt after wait |
| `expr` | **WORKS** | `works` | expr: too few arguments |
| `factor` | **HANG** | `hang_timeout` | no prompt after wait |
| `fallocate` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `false` | **WORKS** | `works` |  |
| `fatattr` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `fbset` | **CRASH** | `crash_panic` | panic |
| `fbsplash` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `fdflush` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `fdisk` | **CRASH** | `crash_panic` | panic |
| `fgrep` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `find` | **WORKS** | `works` | ./bin/true |
| `findfs` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `flock` | **CRASH** | `crash_panic` | panic |
| `fold` | **HANG** | `hang_timeout` | no prompt after wait |
| `free` | **CRASH** | `crash_panic` | panic |
| `fsck` | **CRASH** | `crash_panic` | panic |
| `fstrim` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `fsync` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `fuser` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `getopt` | **WORKS** | `works_error` | getopt: missing optstring argument |
| `getty` | **CRASH** | `crash_panic` | panic |
| `grep` | **WORKS** | `works` | -E    PATTERN is an extended regexp |
| `groups` | **CRASH** | `crash_panic` | panic |
| `gunzip` | **HANG** | `hang_timeout` | no prompt after wait |
| `gzip` | **HANG** | `hang_timeout` | no prompt after wait |
| `halt` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=35 (-38) |
| `hd` | **HANG** | `hang_timeout` | no prompt after wait |
| `head` | **HANG** | `hang_likely` | no prompt after wait |
| `hexdump` | **HANG** | `hang_likely` | no prompt after wait |
| `hostid` | **WORKS** | `works` | 00000000 |
| `hostname` | **WORKS** | `works` | munux |
| `hwclock` | **CRASH** | `crash_panic` | panic |
| `id` | **WORKS** | `works` | uid=0 |
| `ifconfig` | **CRASH** | `crash_panic` | panic |
| `ifdown` | **CRASH** | `crash_panic` | panic |
| `ifenslave` | **CRASH** | `crash_panic` | panic |
| `ifup` | **CRASH** | `crash_panic` | panic |
| `init` | **WORKS** | `works` | init: must be run as PID 1 |
| `inotifyd` | **WORKS** | `works` | If watching a directory: |
| `insmod` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `install` | **CRASH** | `crash_panic` | panic |
| `ionice` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=252 (-38) |
| `iostat` | **CRASH** | `crash_panic` | panic |
| `ip` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `ipaddr` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=41 (-38) |
| `ipcalc` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `ipcrm` | **WORKS** | `works` | (no output, clean exit) |
| `ipcs` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=71 (-38) |
| `iplink` | **CRASH** | `crash_panic` | panic |
| `ipneigh` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=41 (-38) |
| `iproute` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=41 (-38) |
| `iprule` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=41 (-38) |
| `iptunnel` | **CRASH** | `crash_panic` | panic |
| `kbd_mode` | **CRASH** | `crash_panic` | panic |
| `kill` | **WORKS** | `works` | kill: you need to specify whom to kill |
| `killall` | **WORKS** | `works` | killall: you need to specify whom to kill |
| `killall5` | **CRASH** | `crash_panic` | panic |
| `klogd` | **CRASH** | `crash_panic` | panic |
| `last` | **CRASH** | `crash_panic` | panic |
| `less` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=40 (-38) |
| `link` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `linux32` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=135 (-38) |
| `linux64` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=135 (-38) |
| `ln` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `loadfont` | **HANG** | `hang_timeout` | no prompt after wait |
| `loadkmap` | **CRASH** | `crash_panic` | panic |
| `logger` | **HANG** | `hang_timeout` | no prompt after wait |
| `login` | **WORKS** | `works` | (no output, clean exit) |
| `logread` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=29 (-38) |
| `losetup` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `ls` | **WORKS** | `works` | lists root |
| `lsattr` | **CRASH** | `crash_panic` | panic |
| `lsmod` | **CRASH** | `crash_panic` | panic |
| `lsof` | **CRASH** | `crash_panic` | panic |
| `lsusb` | **CRASH** | `crash_panic` | panic |
| `lzcat` | **HANG** | `hang_likely` | no prompt after wait |
| `lzma` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `lzop` | **UNCLEAR** | `unknown` | 'utf-8' codec can't decode byte 0x89 in position 1467: inval |
| `lzopcat` | **HANG** | `hang_likely` | no prompt after wait |
| `makemime` | **HANG** | `hang_timeout` | Mime-Version: 1.0 |
| `md5sum` | **HANG** | `hang_timeout` | no prompt after wait |
| `mdev` | **CRASH** | `crash_panic` | panic |
| `mesg` | **WORKS** | `works_error` | mesg: not a tty |
| `microcom` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `mkdir` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `mkdosfs` | **CRASH** | `crash_panic` | panic |
| `mkfifo` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `mkfs.vfat` | **UNCLEAR** | `unknown` |  |
| `mknod` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `mkpasswd` | **HANG** | `hang_timeout` | no prompt after wait |
| `mkswap` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `mktemp` | **CRASH** | `crash_panic` | panic |
| `modinfo` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `modprobe` | **CRASH** | `crash_panic` | panic |
| `more` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=40 (-38) |
| `mount` | **CRASH** | `crash_panic` | panic |
| `mountpoint` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `mpstat` | **CRASH** | `crash_panic` | panic |
| `mv` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `nameif` | **CRASH** | `crash_panic` | panic |
| `nanddump` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `nandwrite` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `nbd-client` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `nc` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=13 (-38) |
| `netstat` | **CRASH** | `crash_panic` | panic |
| `nice` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=140 (-38) |
| `nl` | **HANG** | `hang_timeout` | no prompt after wait |
| `nmeter` | **CRASH** | `crash_panic` | panic |
| `nohup` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `nologin` | **CRASH** | `crash_panic` | panic |
| `nproc` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=204 (-38) |
| `nsenter` | **WORKS** | `works` | munux sh  \|  help  exit  cd  pwd  vi  \|  fork/exec /bin/<cmd |
| `nslookup` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `ntpd` | **CRASH** | `crash_panic` | panic |
| `od` | **HANG** | `hang_likely` | no prompt after wait |
| `openvt` | **CRASH** | `crash_panic` | panic |
| `partprobe` | **WORKS** | `works` | (no output, clean exit) |
| `passwd` | **CRASH** | `crash_panic` | panic |
| `paste` | **HANG** | `hang_timeout` | no prompt after wait |
| `pgrep` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `pidof` | **WORKS** | `works` | (no output, clean exit) |
| `ping` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `ping6` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `pipe_progress` | **CRASH** | `crash_panic` | panic |
| `pivot_root` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `pkill` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `pmap` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `poweroff` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=35 (-38) |
| `printenv` | **WORKS** | `works` | (no output, clean exit) |
| `printf` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `ps` | **CRASH** | `crash_panic` | panic |
| `pscan` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `pstree` | **CRASH** | `crash_panic` | panic |
| `pwd` | **WORKS** | `works` | / |
| `pwdx` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `raidautorun` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `rdate` | **CRASH** | `crash_panic` | panic |
| `rdev` | **WORKS** | `works` | (no output, clean exit) |
| `readahead` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `readlink` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `realpath` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `reboot` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=35 (-38) |
| `reformime` | **HANG** | `hang_timeout` | no prompt after wait |
| `remove-shell` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=89 (-38) |
| `renice` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `reset` | **WORKS** | `works` | (no output, clean exit) |
| `resize` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=13 (-38) \| syscall: ENOSYS n=13 (-38) \| sy |
| `rev` | **HANG** | `hang_timeout` | no prompt after wait |
| `rfkill` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `rm` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `rmdir` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `rmmod` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `route` | **CRASH** | `crash_panic` | panic |
| `run-parts` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `sed` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `sendmail` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=33 (-38) |
| `seq` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `setconsole` | **CRASH** | `crash_panic` | panic |
| `setfont` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `setkeycodes` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `setlogcons` | **CRASH** | `crash_panic` | panic |
| `setpriv` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `setserial` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `setsid` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `sh` | **CRASH** | `crash_panic` | panic |
| `sha1sum` | **HANG** | `hang_timeout` | no prompt after wait |
| `sha256sum` | **HANG** | `hang_timeout` | no prompt after wait |
| `sha3sum` | **HANG** | `hang_timeout` | no prompt after wait |
| `sha512sum` | **HANG** | `hang_timeout` | no prompt after wait |
| `showkey` | **WORKS** | `works_error` | showkey: can't tcsetattr for stdin: Not a tty |
| `shred` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `shuf` | **HANG** | `hang_timeout` | no prompt after wait |
| `slattach` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `sleep` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `sort` | **HANG** | `hang_timeout` | no prompt after wait |
| `split` | **HANG** | `hang_timeout` | no prompt after wait |
| `ssl_client` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `stat` | **CRASH** | `crash_panic` | panic |
| `strings` | **HANG** | `hang_timeout` | no prompt after wait |
| `stty` | **WORKS** | `works_error` | stty: standard input: Not a tty |
| `su` | **CRASH** | `crash_panic` | panic |
| `sum` | **HANG** | `hang_timeout` | no prompt after wait |
| `swapoff` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `swapon` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `switch_root` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `sync` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=162 (-38) |
| `sysctl` | **CRASH** | `crash_panic` | panic |
| `syslogd` | **CRASH** | `crash_panic` | panic |
| `tac` | **HANG** | `hang_timeout` | no prompt after wait |
| `tail` | **HANG** | `hang_likely` | no prompt after wait |
| `tar` | **WORKS** | `works` | -X FILE    File with glob patterns to exclude |
| `tee` | **HANG** | `hang_timeout` | no prompt after wait |
| `test` | **WORKS** | `works` | (no output, clean exit) |
| `time` | **CRASH** | `crash_panic` | panic |
| `timeout` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `top` | **CRASH** | `crash_panic` | panic |
| `touch` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `tr` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `traceroute` | **CRASH** | `crash_panic` | panic |
| `traceroute6` | **CRASH** | `crash_panic` | panic |
| `tree` | **CRASH** | `crash_panic` | panic |
| `true` | **WORKS** | `works` |  |
| `truncate` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `tty` | **WORKS** | `works_error` | not a tty |
| `ttysize` | **WORKS** | `works` | 80 24 |
| `tunctl` | **CRASH** | `crash_panic` | panic |
| `udhcpc` | **CRASH** | `crash_panic` | panic |
| `udhcpc6` | **CRASH** | `crash_panic` | panic |
| `umount` | **CRASH** | `crash_panic` | panic |
| `uname` | **WORKS** | `works` | munux |
| `unexpand` | **HANG** | `hang_timeout` | no prompt after wait |
| `uniq` | **HANG** | `hang_timeout` | no prompt after wait |
| `unix2dos` | **HANG** | `hang_timeout` | no prompt after wait |
| `unlink` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `unlzma` | **HANG** | `hang_likely` | no prompt after wait |
| `unlzop` | **HANG** | `hang_timeout` | no prompt after wait |
| `unshare` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=272 (-38) |
| `unxz` | **HANG** | `hang_timeout` | no prompt after wait |
| `unzip` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `uptime` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=99 (-38) |
| `usleep` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `uudecode` | **HANG** | `hang_timeout` | no prompt after wait |
| `uuencode` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=95 (-38) |
| `vconfig` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `vi` | **FAIL_ENOSYS** | `enosys` | [?1049h[999;999H[6n[6nsyscall: ENOSYS n=7 (-38) |
| `vlock` | **CRASH** | `crash_panic` | panic |
| `volname` | **CRASH** | `crash_panic` | panic |
| `watch` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `watchdog` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |
| `wc` | **HANG** | `hang_timeout` | no prompt after wait |
| `wget` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=13 (-38) |
| `which` | **CRASH** | `crash_panic` | panic |
| `who` | **FAIL_ENOSYS** | `enosys` | syscall: ENOSYS n=41 (-38) |
| `whoami` | **CRASH** | `crash_panic` | panic |
| `whois` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-call binary. |
| `xargs` | **HANG** | `hang_timeout` | no prompt after wait |
| `xxd` | **HANG** | `hang_likely` | no prompt after wait |
| `xzcat` | **HANG** | `hang_likely` | no prompt after wait |
| `yes` | **HANG** | `hang_likely` | y |
| `zcat` | **HANG** | `hang_likely` |  |
| `zcip` | **WORKS** | `works_usage` | BusyBox v1.36.1 (2025-11-23 14:32:18 UTC) multi-ca |

## Suggested next fixes (from this matrix)

1. **`pipe` / `dup2`** — unlock filters used in pipelines and some applets (`sendmail` already wants dup).
2. **`readlink` / `readlinkat`** — shell helpers, busybox `readlink`.
3. **`socket` family** — any network applet.
4. **`rt_sigaction`** — many C library init paths and shells.
5. **`sendfile`** — `cat`/`head` performance path (busybox may fall back or hang).
6. **Investigate CRASH set** (`df`,`free`,`ps`,`ifconfig`,…) — often `/proc` or bad pointer after failed syscall.
7. **Freestanding sh argv splitting** — so `busybox cat hello.txt` can be tested properly.

