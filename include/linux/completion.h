/* munux linuxkpi — completion (poll + IRQ wake). */
#ifndef _LINUX_COMPLETION_H
#define _LINUX_COMPLETION_H

#include <linux/types.h>

struct completion {
	volatile unsigned int done;
};

static inline void init_completion(struct completion *x)
{
	x->done = 0;
}

static inline void reinit_completion(struct completion *x)
{
	x->done = 0;
}

void complete(struct completion *x);
void wait_for_completion(struct completion *x);
unsigned long wait_for_completion_timeout(struct completion *x,
					  unsigned long timeout);

#endif /* _LINUX_COMPLETION_H */
