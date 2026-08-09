# Linux driver sources on munux (linuxkpi)

**Status:** L0–L5 + virtio-net **done** (2026-08-09). `/dev/vda` + ICMP ping via virtio-net. Next: sockets (P12) or P9 userspace.  
**Success bar (chosen):** **compile Linux driver `.c` sources** against munux headers, `insmod` the resulting ELF `.ko`, and have the device work.  
**Not the bar:** drop a prebuilt Ubuntu/Fedora `.ko` into `/lib/modules` and have it load. That needs one exact Linux kernel ABI (vermagic + thousands of `EXPORT_SYMBOL`s + identical struct layouts).

Think **FreeBSD linuxkpi**, not “become Linux.” We implement the **Linux kernel C API** that drivers call. Internals stay Rust. Results match Linux drivers.

Related: [ROADMAP.md](ROADMAP.md) · [SMOKE_MODULE.md](SMOKE_MODULE.md) · `src/module/`

---

## 1. Why today’s modules cannot run Linux drivers

| Layer | munux today | What a Linux driver expects |
|-------|-------------|-----------------------------|
| Container | ELF64 ET_REL **or** custom **MNX1** | ELF64 ET_REL from **gcc + modpost** |
| Init symbols | `init_module` / `cleanup_module` | Same names, usually from `module_init()` + generated `.mod.c` |
| Exports | `munux_*` + L1 Linux names (`printk`, `kmalloc`, `memcpy`, …). Not `cdev_add` / PCI yet | Linux names: `printk`, `kmalloc`, `cdev_add`, `request_irq`, `pci_register_driver`, … |
| Chardev | Tiny C fops: `read(buf, len)` | `struct file_operations` (`read(file *, char __user *, size_t, loff_t *)`) |
| `THIS_MODULE` | Slot int on chrdev | `struct module *` on `fops.owner` |
| Headers | None (`modules/*.asm`) | `linux/module.h`, `linux/fs.h`, `linux/pci.h`, … |
| Relocs | `R_X86_64_{64,PC32,PLT32,32,32S,GOTPCREL*}` + trampolines + GOT | gcc also emits more types on fat drivers |
| Limits | 48 sections, 512 syms, 1024 relas, 128 KiB ELF image | Some real drivers still exceed this |
| IRQ / PCI / MMIO / DMA | Built-in IDE + PIC keyboard only | `request_irq`, `ioremap`, `pci_*`, `dma_alloc_coherent` |
| Sync | None exported | `spinlock_t`, `mutex`, wait queues, `jiffies`, `msleep` |

The **admin path** already matches Linux (`init_module` / `finit_module` / `delete_module`, `/proc/modules`, `/bin/insmod`). The **driver ABI** does not.

MNX1 stays as a **legacy** format until linuxkpi hello + chardev are green. New work does not extend MNX1.

---

## 2. Strategy

```text
  Linux driver .c
        │  #include <linux/…>     ← headers WE write (compatible API)
        ▼
  gcc -ffreestanding -c  →  ELF64 ET_REL .ko
        │  undef symbols: printk, kmalloc, cdev_add, …
        ▼
  munux loader (grow today’s elfrel.rs)
        │  resolve via EXPORT_SYMBOL table (Linux names)
        ▼
  linuxkpi (Rust, extern "C")  →  real munux VFS / IRQ / PCI / heap
```

**Rules:**

1. **Write our own `include/linux/*.h`.** Do not vendor the Linux kernel tree (license + 10k headers). Match *signatures and fields drivers actually touch*, like FreeBSD linuxkpi.
2. **Implement in Rust** behind `extern "C"` + `#[no_mangle]`. Modules never depend on the Rust crate ABI.
3. **Grow the API by linking a real driver and stubbing the next missing symbol** — not by copying `linux/kernel.h` wholesale.
4. **One driver class at a time.** Chardev first (we already have `/dev`). Then IRQ + MMIO. Then one QEMU device (virtio or e1000). Net/DRM wait until those exist for the desktop.
5. **Binary distro `.ko` remains out of scope** until (if ever) this API is large and layout-stable. Even then, only `.ko` files **built for munux headers** are promised.

---

## 3. Hard technical constraints

### 3.1 Code model (PC32)

Linux builds modules with `-mcmodel=kernel` and maps them **near** kernel text so `call printk` fits in 32-bit PC-relative.

munux loads modules on the **high heap** (`0x1_0000_0000`). Kernel text is around `0x100000`. PC32 cannot reach → today’s **abs64 trampolines**.

| Option | When |
|--------|------|
| Keep trampolines; compile modules `-mcmodel=large` / allow GOT | **L0–L2** (now) |
| Module VA window next to kernel (or higher-half kernel) | **L4+** before a fat virtio/e1000 `.ko` |

Do not start L5 (real PCI driver) until either trampolines scale (GOT + many stubs) or a near-kernel module area exists. See [MM.md](MM.md).

### 3.2 `copy_to_user` / sleep

Linux `file_operations` take `__user` pointers and may sleep. munux syscalls already have a user buffer in the kernel. linuxkpi `read`/`write` must use the **same** copy helpers as `sys_read` (fault → `-EFAULT`), not raw `rep movsb` from a module.

### 3.3 Preempt / IRQ context

Linux drivers assume `spin_lock_irqsave`, “can sleep here / cannot sleep there.” munux nest policy is depth ≤ 1. linuxkpi locks start as **IRQ-disable + flag**; wait queues reuse the existing sleep/`wake_up` path. Document that in-IRQ code must not sleep — same rule as Linux.

### 3.4 License of headers

munux headers are **original** compatible declarations. Do not paste Linux `uapi`/`linux/*.h` into this tree. Driver **sources** you compile (hello, echo, later virtio) keep **their** license (GPL for real Linux files). Keep third-party driver `.c` in a clearly marked directory.

---

## 4. Phases

Work **alongside** Phase 9 (syscalls toward a desktop). linuxkpi is how we get virtio/net/gpu drivers later; it does not replace `execveat` / dynlink.

### L0 — Loader can host a gcc ET_REL ✅

**Why first:** a Linux-looking `.c` will not even relocate in today’s loader.

- Raise limits (sections / symbols / relocs / image size) or make them heap-dynamic.
- Relocs: keep current set; add **`R_X86_64_GOTPCREL`** (and `REX_GOTPCRELX` if gcc emits it) with a per-image GOT.
- Ignore unknown **non-ALLOC** sections (`.note.gnu.build-id`, `.comment`, `__versions`).
- Parse `.modinfo` `name=` (done) and `license=` (store; no GPL enforcement yet).
- Still accept classic `init_module` / `cleanup_module`.
- Keep MNX1 working (no break).

**Exit:** `modules/linux/hello.c` compiled with host `gcc -ffreestanding -c` loads via `insmod` and prints (may still call `printk` only after L1 — until then, a gcc module that only `return 0` proves relocs). ✅ (with L1)

### L1 — Core linuxkpi (printk / slab / module macros) ✅

New tree:

```text
include/linux/          types.h errno.h kernel.h printk.h slab.h
                        string.h module.h init.h
src/linuxkpi/           Rust extern "C" implementations
modules/linux/hello.c   MODULE_LICENSE + module_init printk
```

Macros (good enough; skip real `.mod.c` / initcall sections at first):

```c
#define module_init(fn) int init_module(void) { return fn(); }
#define module_exit(fn) void cleanup_module(void) { fn(); }
```

Export **Linux names** (`printk`, `kmalloc`, `kzalloc`, `kfree`, `memcpy`, …). Keep `munux_*` as aliases so `hello.ko.asm` still links.

**Exit:** `insmod /lib/modules/hello_c.ko` → `hello_c: linuxkpi module loaded` via `printk`; `rmmod hello_c` calls `cleanup_module`. ✅ qemu-connect 2026-08-09.

### L2 — Linux char devices (rewrite echo) ✅

Need:

- `struct file`, `struct inode` (minimal fields)
- `struct file_operations` with Linux `read`/`write`/`open`/`release`/`llseek`
- `copy_to_user` / `copy_from_user`
- `register_chrdev` **or** `misc_register` / `cdev_init`+`cdev_add` (pick **misc_register** first — one minor, one name)
- VFS: `FOPS_MOD` dispatches to Linux fops, not `(buf, len)` only
- `THIS_MODULE` / `try_module_get` / `module_put` so `rmmod` stays EBUSY while open

**Exit:**

1. `modules/linux/echo.c` written as a Linux `miscdevice`. ✅ `/lib/modules/echo_c.ko`
2. `echotest` PASSes on `echo_c.ko` (name=`echo`, `/dev/echo`, EBUSY while open). ✅
3. NASM `echo.ko` / `echo.mnx` still work if `echo_c` is not loaded (same `/dev/echo`). ✅
4. Smoke: [SMOKE_MODULE.md](SMOKE_MODULE.md) updated.

After L2, **new modules are C + linuxkpi only.** Do not load NASM `echo.ko` and `echo_c.ko` at the same time.

### L3 — Sync + IRQ ✅

- `spinlock_t`, `mutex`, `complete` / `wait_for_completion(_timeout)`
- `jiffies` (exported data symbol), `msleep`, `HZ=100`
- `request_irq` / `free_irq`: IRQ0/1 chained (timer/kbd stay); IRQ2–15 get PIC stubs + unmask

**Exit:** `insmod /lib/modules/irqtest.ko` → `irqtest: got IRQ0 (timer) PASS`. ✅ qemu-connect 2026-08-09.

### L4 — MMIO + bus probe ✅

- `ioremap` / `iounmap`, `readl` / `writel` / `ioread32` / `iowrite32`
- Identity-map or dedicated MMIO PTEs (today’s kernel window is low identity — PCI BARs may need explicit maps)
- **Virtio-mmio** first (QEMU `-device virtio-blk-device,disable-legacy=on`) **or** a minimal PCI enum (`pci_read_config_*`, `pci_enable_device`, `pci_iomap`)
- `dma_alloc_coherent`: start with **identity-capable** pages (no IOMMU)

**Exit:** munux sees PCI devices and a linuxkpi probe runs. ✅  
`insmod /lib/modules/vprobe.ko` → Intel/VGA (and **virtio** when QEMU has `-device virtio-blk-pci`, as in `make run`). qemu-connect (IDE-only) still PASSes on i440FX.

### L5 — First real Linux driver ✅

`modules/linux/virtio_blk.c` — modern virtio-pci blk using linuxkpi (`pci_*`, `dma_alloc_coherent`, `request_irq`, `munux_add_disk`). Not a paste of mainline `virtio_blk.c` (blk-mq), but the same PCI/virtqueue/request contract.

**Exit:** `insmod /lib/modules/virtio_blk.ko` → `/dev/vda` readable. ✅ `vdatest: PASS` with QEMU `-device virtio-blk-pci` (`make run` attaches `build/vda.img`). qemu-connect (IDE-only) prints `virtio_blk: no virtio-blk PCI device` and init fails — expected.

### After L5 (desktop path)

| Need | Uses linuxkpi |
|------|----------------|
| P12 networking | virtio-net + a munux `struct net_device` shim |
| P13 graphics | DRM/KMS is a **large** linuxkpi (do framebuffer/Bochs first if DRM is years out) |
| P14 install | virtio-blk + virtio-net + a display path |

---

## 5. Suggested tree / build

```text
include/linux/*.h          compatible headers (munux-owned)
src/linuxkpi/*.rs          C ABI + shims into VFS/IRQ/PCI/heap
src/module/export.rs       name → address (Linux names + munux_* aliases)
src/module/elfrel.rs       loader (L0)
modules/linux/hello.c
modules/linux/echo.c
modules/linux/virtio_blk.c
modules/linux/virtio_net.c
src/net/                   ARP + IPv4 ICMP (ping smoke)
```

Makefile sketch (host gcc, same as other userland tools):

```make
LINUXKPI_CFLAGS := -ffreestanding -fno-stack-protector -fno-pic \
                   -mcmodel=large -mno-red-zone -O2 -Wall \
                   -Iinclude
modules/linux/%.ko: modules/linux/%.c
	$(CC) $(LINUXKPI_CFLAGS) -c -o $@ $<
```

Install onto `build/disk.img` next to `/lib/modules/hello.ko`.

---

## 6. What we rewrite vs keep

| Piece | Action |
|-------|--------|
| `init_module` / `delete_module` / `lsmod` | **Keep** |
| ELF ET_REL loader | **Grow** (L0) |
| `munux_register_chrdev` tiny fops | **Replace** at L2 with Linux fops; alias old API for one cycle |
| MNX1 + `modules/*.asm` | **Keep until L2 green**, then freeze/deprecate |
| IDE `hda` built-in | **Keep** (chicken-egg). Future virtio-blk can be a module if IDE or initrd can still boot |
| Mainline vermagic / ksymtab CRCs / distro `.ko` | **Still out of scope** |

---

## 7. Near-term work order (when implementation starts)

Do **not** pause Phase 9 forever. Interleave:

1. ~~**L0 + L1** loader + printk/kmalloc `hello.c`~~ ✅  
2. ~~**L2** echo as Linux C miscdevice~~ ✅  
3. ~~**L3** spinlock / wait / `request_irq`~~ ✅ `irqtest.ko`  
4. ~~**L4** ioremap + PCI `pci_register_driver`~~ ✅ `vprobe.ko`  
5. ~~**L5 virtio-blk**~~ ✅ `/dev/vda` + `vdatest: PASS`  
6. ~~**P9** `execveat` / `prctl`~~ ✅  
7. ~~virtio-net~~ ✅ `virtio_net.ko` pings 10.0.2.2; sockets still P12  
8. ~~**P9e** file-map ELF + file `MAP_SHARED`~~ ✅  
9. ~~**P10a** `PT_INTERP`~~ ✅  
10. ~~**P10b** `ET_DYN` bias~~ ✅  
11. ~~**P10c** glibc `hello_dyn`~~ ✅ `ld.so` + `libc.so.6`  

---

---

## 8. Risks

| Risk | Mitigation |
|------|------------|
| Infinite header surface | Only add a symbol when a chosen driver fails to compile/link |
| Accidental Linux source paste | Original headers; review diffs for copied comment blocks |
| PC32 / mcmodel | large + trampolines/GOT now; near-kernel module VA before L5 |
| Sleeping in IRQ | linuxkpi `might_sleep` checks; start with spinlocks = cli |
| Scope vs desktop | Chardev + one virtio beats “implement all of `linux/pci.h`” |
| GPL driver objects in the image | Keep Linux `.c` isolated; do not claim munux is a Linux kernel |

---

## 9. Success definition

linuxkpi **works** when:

1. A Linux-style `hello.c` (`module_init` + `printk`) builds with gcc and loads.  
2. A Linux-style char driver replaces our NASM echo and passes `echotest`.  
3. Later: one **upstream** driver (virtio-blk) runs under QEMU.

That is “use Linux modules” in the clang/gcc sense: **same driver source, same userspace result, our kernel.**
