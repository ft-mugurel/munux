/* munux linuxkpi — pci_device_id. */
#ifndef _LINUX_MOD_DEVICETABLE_H
#define _LINUX_MOD_DEVICETABLE_H

#include <linux/types.h>

struct pci_device_id {
	u32 vendor, device;
	u32 subvendor, subdevice;
	u32 class, class_mask;
	unsigned long driver_data;
};

#endif /* _LINUX_MOD_DEVICETABLE_H */
