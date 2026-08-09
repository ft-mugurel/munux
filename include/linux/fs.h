/* munux linuxkpi — minimal file / inode / fops (L2 char devices). */
#ifndef _LINUX_FS_H
#define _LINUX_FS_H

#include <linux/types.h>
#include <linux/module.h>

#ifndef __user
#define __user
#endif

struct file {
	loff_t f_pos;
	void *private_data;
	const struct file_operations *f_op;
	unsigned int f_flags;
};

struct inode {
	unsigned int i_rdev;
};

struct file_operations {
	struct module *owner;
	loff_t (*llseek)(struct file *, loff_t, int);
	ssize_t (*read)(struct file *, char __user *, size_t, loff_t *);
	ssize_t (*write)(struct file *, const char __user *, size_t, loff_t *);
	int (*open)(struct inode *, struct file *);
	int (*release)(struct inode *, struct file *);
};

int register_chrdev(unsigned int major, const char *name,
		    const struct file_operations *fops);
int unregister_chrdev(unsigned int major, const char *name);

#endif /* _LINUX_FS_H */
