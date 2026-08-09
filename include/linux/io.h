/* munux linuxkpi — ioremap / MMIO accessors. */
#ifndef _LINUX_IO_H
#define _LINUX_IO_H

#include <linux/types.h>

#define __iomem

void __iomem *ioremap(unsigned long phys, unsigned long size);
void iounmap(volatile void __iomem *addr);

static inline u32 readl(const volatile void __iomem *addr)
{
	return *(const volatile u32 *)addr;
}

static inline void writel(u32 val, volatile void __iomem *addr)
{
	*(volatile u32 *)addr = val;
}

static inline u32 ioread32(const volatile void __iomem *addr)
{
	return readl(addr);
}

static inline void iowrite32(u32 val, volatile void __iomem *addr)
{
	writel(val, addr);
}

#endif /* _LINUX_IO_H */
