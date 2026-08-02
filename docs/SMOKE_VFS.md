# Phase 7 smoke: VFS (7a + 7b)

## Implemented

| Piece | Notes |
|-------|--------|
| `FileOperations` | console, ext2_file/dir, ramfs, null, zero, **proc** |
| Mount table | `/` → ext2, `/ram` → ramfs, **`/proc` → proc** |
| `register_chrdev` | `/dev/null`, `/dev/zero` |
| `register_blkdev` | **`hda`** (IDE); ext2 I/O via blockdev |
| FD open/read/write | Through `vfs_open` / `vfs_read` / `vfs_write` |
| procfs | `/proc/meminfo`, `mounts`, `version`, `uptime`, `self/status` |

## Full userspace matrix (qemu-connect, freestanding `$`)

All of these were verified **PASS**:

| Command | Expected |
|---------|----------|
| `ls` / `ls /` | includes `proc`, `dev`, `ram` + ext2 names |
| `ls /proc` | `meminfo` `mounts` `version` `uptime` `self` |
| `ls /dev` | `null` `zero` |
| `ls /ram` | `hello` |
| `ls /proc/self` | `status` |
| `cat /proc/meminfo` | MemTotal/Free/Available/Used |
| `cat /proc/mounts` | ext2, ramfs, proc, devtmpfs lines |
| `cat /proc/version` | `munux version 0.3 x86_64` |
| `cat /proc/uptime` | `N.NN N.NN` |
| `cat /proc/self/status` | Name/Pid/Tgid |
| `cat /ram/hello` | `ramfs says hi` |
| `cat /dev/null` | empty, returns to `$` |
| `cat hello.txt` | Hello from munux… |
| `cd /proc` + `pwd` + `ls` + `cat meminfo` | relative open works |
| `cd /dev` + `pwd` + `ls` | null/zero |
| `cd /ram` + `cat hello` | works |
| `cd /` + `pwd` | `/` |

**Known non-bug:** `cat /dev/zero` never ends (infinite zeros), same as Linux. Do not use unbounded `cat` on zero.

## Not yet

- Full dentry/inode cache
- Block-device syscalls / `/dev/hda` node
- `mount`/`umount` syscalls
- Unify all FS mutations (unlink/mkdir) through VFS inode ops
