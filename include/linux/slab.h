/* munux linuxkpi — slab/kmalloc. */
#ifndef _LINUX_SLAB_H
#define _LINUX_SLAB_H

#include <linux/types.h>
#include <linux/gfp.h>

void *kmalloc(unsigned long size, gfp_t flags);
void *kzalloc(unsigned long size, gfp_t flags);
void kfree(const void *ptr);

#endif /* _LINUX_SLAB_H */
