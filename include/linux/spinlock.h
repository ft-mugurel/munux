/* munux linuxkpi — UP spinlocks (cli + flag). */
#ifndef _LINUX_SPINLOCK_H
#define _LINUX_SPINLOCK_H

#include <linux/types.h>

typedef struct spinlock {
	volatile unsigned int locked;
} spinlock_t;

#define DEFINE_SPINLOCK(x) spinlock_t x = { 0 }

static inline void spin_lock_init(spinlock_t *lock)
{
	lock->locked = 0;
}

void spin_lock(spinlock_t *lock);
void spin_unlock(spinlock_t *lock);
unsigned long __spin_lock_irqsave(spinlock_t *lock);
void __spin_unlock_irqrestore(spinlock_t *lock, unsigned long flags);

#define spin_lock_irqsave(lock, flags) \
	do { (flags) = __spin_lock_irqsave(lock); } while (0)
#define spin_unlock_irqrestore(lock, flags) \
	__spin_unlock_irqrestore((lock), (flags))

#endif /* _LINUX_SPINLOCK_H */
