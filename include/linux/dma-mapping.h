/* munux linuxkpi — coherent DMA (identity frames). */
#ifndef _LINUX_DMA_MAPPING_H
#define _LINUX_DMA_MAPPING_H

#include <linux/types.h>
#include <linux/gfp.h>

void *dma_alloc_coherent(unsigned long size, u64 *dma_handle, gfp_t gfp);
void dma_free_coherent(unsigned long size, void *cpu_addr, u64 dma_handle);

#endif /* _LINUX_DMA_MAPPING_H */
