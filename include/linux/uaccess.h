/* munux linuxkpi — copy_{to,from}_user (L2: memcpy; VFS already staged buffers). */
#ifndef _LINUX_UACCESS_H
#define _LINUX_UACCESS_H

#include <linux/types.h>

#define __user

unsigned long copy_to_user(void __user *to, const void *from, unsigned long n);
unsigned long copy_from_user(void *to, const void __user *from, unsigned long n);

#endif /* _LINUX_UACCESS_H */
