/* munux linuxkpi — kernel helpers. */
#ifndef _LINUX_KERNEL_H
#define _LINUX_KERNEL_H

#include <linux/types.h>
#include <linux/printk.h>

#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define likely(x)     (x)
#define unlikely(x)   (x)

#define BUG() do { printk("BUG\n"); } while (0)
#define BUG_ON(x) do { if (x) BUG(); } while (0)

#ifndef min
#define min(a, b) ((a) < (b) ? (a) : (b))
#endif
#ifndef max
#define max(a, b) ((a) > (b) ? (a) : (b))
#endif

#endif /* _LINUX_KERNEL_H */
