/* munux linuxkpi — PCI scan + pci_register_driver (L4). */
#ifndef _LINUX_PCI_H
#define _LINUX_PCI_H

#include <linux/types.h>
#include <linux/mod_devicetable.h>
#include <linux/io.h>

#define PCI_ANY_ID           (~0u)
#define PCI_VENDOR_ID_REDHAT_QUMRANET 0x1af4
#define PCI_VENDOR_ID_INTEL  0x8086

#define PCI_DEVICE(vend, dev) \
	.vendor = (vend), .device = (dev), .subvendor = PCI_ANY_ID, \
	.subdevice = PCI_ANY_ID

struct pci_dev {
	unsigned int bus;
	unsigned int devfn;
	unsigned short vendor;
	unsigned short device;
	unsigned short subsystem_vendor;
	unsigned short subsystem_device;
	unsigned int class;
	unsigned int irq;
};

struct pci_driver {
	const char *name;
	const struct pci_device_id *id_table;
	int (*probe)(struct pci_dev *dev, const struct pci_device_id *id);
	void (*remove)(struct pci_dev *dev);
};

int pci_register_driver(struct pci_driver *drv);
void pci_unregister_driver(struct pci_driver *drv);
int pci_enable_device(struct pci_dev *dev);
void pci_disable_device(struct pci_dev *dev);
int pci_read_config_dword(struct pci_dev *dev, int where, u32 *val);
int pci_write_config_dword(struct pci_dev *dev, int where, u32 val);
void __iomem *pci_iomap(struct pci_dev *dev, int bar, unsigned long max);
void pci_iounmap(struct pci_dev *dev, void __iomem *addr);

#ifndef MODULE_DEVICE_TABLE
#define MODULE_DEVICE_TABLE(type, name)
#endif

#endif /* _LINUX_PCI_H */
