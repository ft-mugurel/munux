/* munux linuxkpi — misc_register → /dev/<name>. */
#ifndef _LINUX_MISCDEVICE_H
#define _LINUX_MISCDEVICE_H

#include <linux/fs.h>

#define MISC_DYNAMIC_MINOR 255

struct miscdevice {
	int minor;
	const char *name;
	const struct file_operations *fops;
};

int misc_register(struct miscdevice *misc);
int misc_deregister(struct miscdevice *misc);

#endif /* _LINUX_MISCDEVICE_H */
