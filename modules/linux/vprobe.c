/* linuxkpi L4: PCI probe smoke.
 * Matches QEMU i440FX/PIIX (always present) and virtio (if attached).
 * Installed as /lib/modules/vprobe.ko
 */
#include <linux/module.h>
#include <linux/pci.h>
#include <linux/errno.h>

static int found;

static int vprobe_probe(struct pci_dev *dev, const struct pci_device_id *id)
{
	(void)id;
	found++;
	if (dev->vendor == 0x1af4)
		printk(KERN_INFO "vprobe: virtio\n");
	else if (dev->vendor == 0x8086)
		printk(KERN_INFO "vprobe: Intel PCI\n");
	else if (dev->vendor == 0x1234)
		printk(KERN_INFO "vprobe: QEMU VGA\n");
	else
		printk(KERN_INFO "vprobe: other PCI\n");
	return 0;
}

static void vprobe_remove(struct pci_dev *dev)
{
	(void)dev;
}

static const struct pci_device_id vprobe_ids[] = {
	{ PCI_DEVICE(0x8086, PCI_ANY_ID) },
	{ PCI_DEVICE(0x1af4, PCI_ANY_ID) },
	{ PCI_DEVICE(0x1234, 0x1111) },
	{ 0, }
};

static struct pci_driver vprobe_drv = {
	.name = "vprobe",
	.id_table = vprobe_ids,
	.probe = vprobe_probe,
	.remove = vprobe_remove,
};

static int vprobe_init(void)
{
	int ret = pci_register_driver(&vprobe_drv);

	if (ret)
		return ret;
	if (!found) {
		pci_unregister_driver(&vprobe_drv);
		printk(KERN_ERR "vprobe: no matching PCI device\n");
		return -ENODEV;
	}
	printk(KERN_INFO "vprobe: PASS\n");
	return 0;
}

static void vprobe_exit(void)
{
	pci_unregister_driver(&vprobe_drv);
	printk(KERN_INFO "vprobe: unloaded\n");
}

module_init(vprobe_init);
module_exit(vprobe_exit);
MODULE_LICENSE("GPL");
MODULE_INFO(name, "vprobe");
MODULE_DESCRIPTION("munux linuxkpi PCI probe");
MODULE_DEVICE_TABLE(pci, vprobe_ids);
