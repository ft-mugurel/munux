/* munux linuxkpi — request_irq / free_irq. */
#ifndef _LINUX_INTERRUPT_H
#define _LINUX_INTERRUPT_H

#include <linux/types.h>

#define IRQF_SHARED 0x00000080

#define IRQ_NONE    0
#define IRQ_HANDLED 1

typedef int irqreturn_t;
typedef irqreturn_t (*irq_handler_t)(int, void *);

int request_irq(unsigned int irq, irq_handler_t handler, unsigned long flags,
		const char *name, void *dev);
void free_irq(unsigned int irq, void *dev);

#endif /* _LINUX_INTERRUPT_H */
