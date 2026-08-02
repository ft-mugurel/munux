# Phase 7 smoke: VFS (7a)

## Implemented

| Piece | Notes |
|-------|--------|
| `FileOperations` | Static fops table: console, ext2_file/dir, ramfs, null, zero |
| Mount table | `/` → ext2, `/ram` → ramfs |
| `register_chrdev` | `/dev/null`, `/dev/zero` |
| FD open/read/write | Through `vfs_open` / `vfs_read` / `vfs_write` |

## Quick checks

```text
$ cat hello.txt          # ext2 via VFS
$ exit
munux> vfs               # mounts=2 chrdev=2
```

Userspace (when tools open these paths):

- `/dev/null` — write discards; read returns 0
- `/dev/zero` — read fills zeros
- `/ram/hello` — seeded ramfs file

## Not yet

- Full dentry/inode cache
- `/proc` behind VFS ops
- Block-device registration for IDE
- `mount`/`umount` syscalls
