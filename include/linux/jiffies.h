/* munux linuxkpi — jiffies / HZ (PIT 100 Hz). */
#ifndef _LINUX_JIFFIES_H
#define _LINUX_JIFFIES_H

#include <linux/types.h>

#define HZ 100

extern unsigned long jiffies;

#define time_after(a, b)  ((long)((b) - (a)) < 0)
#define time_before(a, b) time_after(b, a)

static inline unsigned long msecs_to_jiffies(const unsigned int m)
{
	return ((unsigned long)(m) + (1000 / HZ) - 1) / (1000 / HZ);
}

#endif /* _LINUX_JIFFIES_H */
