/* munux linuxkpi — register a named disk (/dev/<name>). */
#ifndef _LINUX_BLKDEV_H
#define _LINUX_BLKDEV_H

#include <linux/types.h>

int munux_add_disk(const char *name, unsigned int nsectors,
		   int (*bread)(unsigned int lba, unsigned int count, void *buf),
		   int (*bwrite)(unsigned int lba, unsigned int count, void *buf));
int munux_del_disk(const char *name);

#endif /* _LINUX_BLKDEV_H */
