/* munux linuxkpi — ioremap / MMIO accessors. */
#ifndef _LINUX_IO_H
#define _LINUX_IO_H

#include <linux/types.h>

#define __iomem

void __iomem *ioremap(unsigned long phys, unsigned long size);
void iounmap(volatile void __iomem *addr);

static inline u8 readb(const volatile void __iomem *addr)
{
	return *(const volatile u8 *)addr;
}

static inline void writeb(u8 val, volatile void __iomem *addr)
{
	*(volatile u8 *)addr = val;
}

static inline u16 readw(const volatile void __iomem *addr)
{
	return *(const volatile u16 *)addr;
}

static inline void writew(u16 val, volatile void __iomem *addr)
{
	*(volatile u16 *)addr = val;
}

static inline u32 readl(const volatile void __iomem *addr)
{
	return *(const volatile u32 *)addr;
}

static inline void writel(u32 val, volatile void __iomem *addr)
{
	*(volatile u32 *)addr = val;
}

static inline void wmb(void)
{
	__asm__ volatile("mfence" ::: "memory");
}

static inline void rmb(void)
{
	__asm__ volatile("lfence" ::: "memory");
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
