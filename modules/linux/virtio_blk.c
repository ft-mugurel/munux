/* linuxkpi L5: modern virtio-pci block → /dev/vda
 * QEMU: -device virtio-blk-pci (make run). qemu-connect is IDE-only → -ENODEV.
 */
#include <linux/module.h>
#include <linux/pci.h>
#include <linux/interrupt.h>
#include <linux/blkdev.h>
#include <linux/dma-mapping.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/errno.h>
#include <linux/jiffies.h>
#include <linux/gfp.h>

#define VIRTIO_PCI_CAP_COMMON_CFG	1
#define VIRTIO_PCI_CAP_NOTIFY_CFG	2
#define VIRTIO_PCI_CAP_ISR_CFG		3
#define VIRTIO_PCI_CAP_DEVICE_CFG	4
#define PCI_CAP_ID_VNDR			0x09

#define VIRTIO_F_VERSION_1		32
#define VIRTIO_CONFIG_S_ACKNOWLEDGE	1
#define VIRTIO_CONFIG_S_DRIVER		2
#define VIRTIO_CONFIG_S_DRIVER_OK	4
#define VIRTIO_CONFIG_S_FEATURES_OK	8
#define VIRTIO_CONFIG_S_FAILED		128

#define VRING_DESC_F_NEXT	1
#define VRING_DESC_F_WRITE	2
#define VIRTIO_BLK_T_IN		0
#define VIRTIO_BLK_T_OUT	1
#define VIRTIO_BLK_S_OK		0
#define QSZ_MAX			64

struct virtio_pci_common_cfg {
	u32 device_feature_select;
	u32 device_feature;
	u32 driver_feature_select;
	u32 driver_feature;
	u16 msix_config;
	u16 num_queues;
	u8 device_status;
	u8 config_generation;
	u16 queue_select;
	u16 queue_size;
	u16 queue_msix_vector;
	u16 queue_enable;
	u16 queue_notify_off;
	u32 queue_desc_lo, queue_desc_hi;
	u32 queue_driver_lo, queue_driver_hi;
	u32 queue_device_lo, queue_device_hi;
} __attribute__((packed));

struct vring_desc {
	u64 addr;
	u32 len;
	u16 flags;
	u16 next;
} __attribute__((packed));

struct vring_avail {
	u16 flags;
	u16 idx;
	u16 ring[QSZ_MAX];
} __attribute__((packed));

struct vring_used_elem {
	u32 id;
	u32 len;
} __attribute__((packed));

struct vring_used {
	u16 flags;
	u16 idx;
	struct vring_used_elem ring[QSZ_MAX];
} __attribute__((packed));

struct virtio_blk_outhdr {
	u32 type;
	u32 ioprio;
	u64 sector;
} __attribute__((packed));

static struct pci_dev *vb_pdev;
static void __iomem *vb_bars[6];
static volatile struct virtio_pci_common_cfg *vb_cfg;
static volatile u8 *vb_notify;
static u32 vb_notify_mult;
static volatile u8 *vb_isr;
static volatile u8 *vb_devcfg;
static u16 vb_qsz;
static u16 vb_last_used;
static void *desc_cpu, *avail_cpu, *used_cpu, *req_cpu;
static u64 desc_dma, avail_dma, used_dma, req_dma;
static int vb_ready;
static unsigned int vb_nsectors;

static void *map_cap(struct pci_dev *pdev, u8 pos, u32 *len_out)
{
	u8 bar;
	u32 off, len;

	pci_read_config_byte(pdev, pos + 4, &bar);
	pci_read_config_dword(pdev, pos + 8, &off);
	pci_read_config_dword(pdev, pos + 12, &len);
	if (bar > 5 || !len)
		return NULL;
	if (!vb_bars[bar])
		vb_bars[bar] = pci_iomap(pdev, bar, 0x10000);
	if (!vb_bars[bar])
		return NULL;
	if (len_out)
		*len_out = len;
	return (u8 *)vb_bars[bar] + off;
}

static int find_caps(struct pci_dev *pdev)
{
	u8 pos;
	u8 next;

	pos = pci_find_capability(pdev, PCI_CAP_ID_VNDR);
	while (pos) {
		u8 type;

		pci_read_config_byte(pdev, pos + 3, &type);
		if (type == VIRTIO_PCI_CAP_COMMON_CFG)
			vb_cfg = map_cap(pdev, pos, NULL);
		else if (type == VIRTIO_PCI_CAP_NOTIFY_CFG) {
			u32 dummy;

			vb_notify = map_cap(pdev, pos, &dummy);
			pci_read_config_dword(pdev, pos + 16, &vb_notify_mult);
			if (vb_notify_mult == 0)
				vb_notify_mult = 1;
		} else if (type == VIRTIO_PCI_CAP_ISR_CFG)
			vb_isr = map_cap(pdev, pos, NULL);
		else if (type == VIRTIO_PCI_CAP_DEVICE_CFG)
			vb_devcfg = map_cap(pdev, pos, NULL);
		pci_read_config_byte(pdev, pos + 1, &next);
		pos = next & 0xFC;
		if (pos < 0x40)
			break;
	}
	return (vb_cfg && vb_notify && vb_devcfg) ? 0 : -ENODEV;
}

static void set_status(u8 s)
{
	writeb(s, (void *)&vb_cfg->device_status);
	wmb();
}

static int setup_queue(void)
{
	u16 qsz;
	u16 noff;

	writew(0, (void *)&vb_cfg->queue_select);
	writew(0xFFFF, (void *)&vb_cfg->msix_config);
	writew(0xFFFF, (void *)&vb_cfg->queue_msix_vector);
	qsz = readw((void *)&vb_cfg->queue_size);
	if (qsz == 0)
		return -ENODEV;
	if (qsz > QSZ_MAX)
		qsz = QSZ_MAX;
	vb_qsz = qsz;
	writew(qsz, (void *)&vb_cfg->queue_size);

	desc_cpu = dma_alloc_coherent(4096, &desc_dma, GFP_KERNEL);
	avail_cpu = dma_alloc_coherent(4096, &avail_dma, GFP_KERNEL);
	used_cpu = dma_alloc_coherent(4096, &used_dma, GFP_KERNEL);
	req_cpu = dma_alloc_coherent(4096, &req_dma, GFP_KERNEL);
	if (!desc_cpu || !avail_cpu || !used_cpu || !req_cpu)
		return -ENOMEM;
	memset(desc_cpu, 0, 4096);
	memset(avail_cpu, 0, 4096);
	memset(used_cpu, 0, 4096);
	memset(req_cpu, 0, 4096);

	writel((u32)desc_dma, (void *)&vb_cfg->queue_desc_lo);
	writel((u32)(desc_dma >> 32), (void *)&vb_cfg->queue_desc_hi);
	writel((u32)avail_dma, (void *)&vb_cfg->queue_driver_lo);
	writel((u32)(avail_dma >> 32), (void *)&vb_cfg->queue_driver_hi);
	writel((u32)used_dma, (void *)&vb_cfg->queue_device_lo);
	writel((u32)(used_dma >> 32), (void *)&vb_cfg->queue_device_hi);
	wmb();
	writew(1, (void *)&vb_cfg->queue_enable);
	noff = readw((void *)&vb_cfg->queue_notify_off);
	vb_notify += (unsigned long)noff * vb_notify_mult;
	vb_last_used = 0;
	return 0;
}

static int negotiate(void)
{
	u32 hi;

	set_status(0);
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE);
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE | VIRTIO_CONFIG_S_DRIVER);

	writel(1, (void *)&vb_cfg->device_feature_select);
	hi = readl((void *)&vb_cfg->device_feature);
	if ((hi & 1) == 0) {
		set_status(VIRTIO_CONFIG_S_FAILED);
		return -ENODEV;
	}
	writel(0, (void *)&vb_cfg->driver_feature_select);
	writel(0, (void *)&vb_cfg->driver_feature);
	writel(1, (void *)&vb_cfg->driver_feature_select);
	writel(1, (void *)&vb_cfg->driver_feature); /* VIRTIO_F_VERSION_1 */
	wmb();
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE | VIRTIO_CONFIG_S_DRIVER |
		   VIRTIO_CONFIG_S_FEATURES_OK);
	if (!(readb((void *)&vb_cfg->device_status) & VIRTIO_CONFIG_S_FEATURES_OK)) {
		set_status(VIRTIO_CONFIG_S_FAILED);
		return -ENODEV;
	}
	return 0;
}

static int vb_xfer(int write, unsigned int lba, unsigned int count, void *buf)
{
	struct vring_desc *desc = desc_cpu;
	struct vring_avail *avail = avail_cpu;
	volatile struct vring_used *used = used_cpu;
	struct virtio_blk_outhdr *hdr = req_cpu;
	u8 *data = (u8 *)req_cpu + 64;
	u8 *status = (u8 *)req_cpu + 64 + 512;
	u16 aidx;
	unsigned long t0;
	unsigned int n = count * 512;
	int i;

	if (!vb_ready || n == 0 || n > 512)
		return -EINVAL;
	if (write)
		memcpy(data, buf, n);
	else
		memset(data, 0, n);

	hdr->type = write ? VIRTIO_BLK_T_OUT : VIRTIO_BLK_T_IN;
	hdr->ioprio = 0;
	hdr->sector = lba;
	*status = 0xFF;

	desc[0].addr = req_dma;
	desc[0].len = 16;
	desc[0].flags = VRING_DESC_F_NEXT;
	desc[0].next = 1;
	desc[1].addr = req_dma + 64;
	desc[1].len = n;
	desc[1].flags = VRING_DESC_F_NEXT | (write ? 0 : VRING_DESC_F_WRITE);
	desc[1].next = 2;
	desc[2].addr = req_dma + 64 + 512;
	desc[2].len = 1;
	desc[2].flags = VRING_DESC_F_WRITE;
	desc[2].next = 0;

	aidx = avail->idx;
	avail->ring[aidx % vb_qsz] = 0;
	wmb();
	avail->idx = aidx + 1;
	wmb();
	writew(0, (void *)vb_notify);

	t0 = jiffies;
	for (i = 0; i < 1000000; i++) {
		rmb();
		if (used->idx != vb_last_used)
			break;
		if (jiffies - t0 > HZ)
			return -EIO;
		__asm__ volatile("pause");
	}
	if (used->idx == vb_last_used)
		return -EIO;
	vb_last_used = used->idx;
	if (*status != VIRTIO_BLK_S_OK)
		return -EIO;
	if (!write)
		memcpy(buf, data, n);
	return 0;
}

static int vb_bread(unsigned int lba, unsigned int count, void *buf)
{
	return vb_xfer(0, lba, count, buf);
}

static int vb_bwrite(unsigned int lba, unsigned int count, void *buf)
{
	return vb_xfer(1, lba, count, buf);
}

static irqreturn_t vb_irq(int irq, void *dev)
{
	(void)irq;
	(void)dev;
	if (vb_isr)
		(void)readb((void *)vb_isr);
	return IRQ_HANDLED;
}

static void vb_teardown(struct pci_dev *pdev)
{
	if (vb_ready) {
		munux_del_disk("vda");
		vb_ready = 0;
	}
	if (pdev)
		free_irq(pdev->irq, pdev);
	if (desc_cpu)
		dma_free_coherent(4096, desc_cpu, desc_dma);
	if (avail_cpu)
		dma_free_coherent(4096, avail_cpu, avail_dma);
	if (used_cpu)
		dma_free_coherent(4096, used_cpu, used_dma);
	if (req_cpu)
		dma_free_coherent(4096, req_cpu, req_dma);
	desc_cpu = avail_cpu = used_cpu = req_cpu = NULL;
	{
		int b;

		for (b = 0; b < 6; b++) {
			if (vb_bars[b]) {
				pci_iounmap(pdev, vb_bars[b]);
				vb_bars[b] = NULL;
			}
		}
	}
	vb_cfg = NULL;
	vb_notify = NULL;
	vb_isr = NULL;
	vb_devcfg = NULL;
}

static int vb_probe(struct pci_dev *pdev, const struct pci_device_id *id)
{
	u32 cap_lo, cap_hi;
	int ret;

	(void)id;
	if (vb_ready)
		return -EBUSY;
	if (pci_enable_device(pdev))
		return -EIO;
	if (find_caps(pdev)) {
		printk(KERN_ERR "virtio_blk: not modern virtio-pci\n");
		return -ENODEV;
	}
	if (negotiate()) {
		printk(KERN_ERR "virtio_blk: feature negotiate failed\n");
		return -ENODEV;
	}
	if (setup_queue()) {
		printk(KERN_ERR "virtio_blk: queue setup failed\n");
		set_status(VIRTIO_CONFIG_S_FAILED);
		vb_teardown(pdev);
		return -ENOMEM;
	}
	cap_lo = readl((void *)vb_devcfg);
	cap_hi = readl((void *)(vb_devcfg + 4));
	vb_nsectors = (unsigned int)cap_lo;
	(void)cap_hi;
	if (vb_nsectors == 0) {
		printk(KERN_ERR "virtio_blk: zero capacity\n");
		vb_teardown(pdev);
		return -ENODEV;
	}
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE | VIRTIO_CONFIG_S_DRIVER |
		   VIRTIO_CONFIG_S_FEATURES_OK | VIRTIO_CONFIG_S_DRIVER_OK);
	if (pdev->irq)
		request_irq(pdev->irq, vb_irq, IRQF_SHARED, "virtio_blk", pdev);
	ret = munux_add_disk("vda", vb_nsectors, vb_bread, vb_bwrite);
	if (ret) {
		vb_teardown(pdev);
		return ret;
	}
	vb_pdev = pdev;
	vb_ready = 1;
	printk(KERN_INFO "virtio_blk: /dev/vda ready\n");
	return 0;
}

static void vb_remove(struct pci_dev *pdev)
{
	vb_teardown(pdev);
	vb_pdev = NULL;
	printk(KERN_INFO "virtio_blk: removed\n");
}

static const struct pci_device_id vb_ids[] = {
	{ PCI_DEVICE(0x1af4, 0x1001) },
	{ PCI_DEVICE(0x1af4, 0x1042) },
	{ 0, }
};

static struct pci_driver vb_drv = {
	.name = "virtio_blk",
	.id_table = vb_ids,
	.probe = vb_probe,
	.remove = vb_remove,
};

static int vb_init(void)
{
	int ret = pci_register_driver(&vb_drv);

	if (ret)
		return ret;
	if (!vb_ready) {
		pci_unregister_driver(&vb_drv);
		printk(KERN_ERR "virtio_blk: no virtio-blk PCI device\n");
		return -ENODEV;
	}
	return 0;
}

static void vb_exit(void)
{
	pci_unregister_driver(&vb_drv);
}

module_init(vb_init);
module_exit(vb_exit);
MODULE_LICENSE("GPL");
MODULE_INFO(name, "virtio_blk");
MODULE_DESCRIPTION("munux linuxkpi virtio-blk");
MODULE_DEVICE_TABLE(pci, vb_ids);
