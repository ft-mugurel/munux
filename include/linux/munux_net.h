/* munux linuxkpi — NIC + ICMP ping (P12 start). */
#ifndef _LINUX_MUNUX_NET_H
#define _LINUX_MUNUX_NET_H

#include <linux/types.h>

int munux_register_nic(const char *name, const u8 mac[6],
		       int (*xmit)(const void *frame, unsigned int len),
		       void (*poll)(void));
void munux_unregister_nic(void);
void munux_netif_rx(const void *frame, unsigned int len);
int munux_icmp_ping(u32 dst_host_order, unsigned long timeout_jiffies);

#endif /* _LINUX_MUNUX_NET_H */
