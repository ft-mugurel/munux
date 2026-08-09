/* munux linuxkpi — string/memory helpers. */
#ifndef _LINUX_STRING_H
#define _LINUX_STRING_H

#include <linux/types.h>

void *memcpy(void *dst, const void *src, unsigned long n);
void *memmove(void *dst, const void *src, unsigned long n);
void *memset(void *dst, int c, unsigned long n);
unsigned long strlen(const char *s);

#endif /* _LINUX_STRING_H */
