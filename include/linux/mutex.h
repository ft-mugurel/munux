/* munux linuxkpi — simple mutex (UP). */
#ifndef _LINUX_MUTEX_H
#define _LINUX_MUTEX_H

#include <linux/spinlock.h>

struct mutex {
	volatile int count; /* 1 = unlocked */
	spinlock_t wait_lock;
};

#define DEFINE_MUTEX(m) struct mutex m = { .count = 1, .wait_lock = { 0 } }

static inline void mutex_init(struct mutex *m)
{
	m->count = 1;
	spin_lock_init(&m->wait_lock);
}

void mutex_lock(struct mutex *m);
void mutex_unlock(struct mutex *m);

#endif /* _LINUX_MUTEX_H */
