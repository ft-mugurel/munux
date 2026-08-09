/* linuxkpi: modern virtio-pci net + ICMP ping smoke (10.0.2.2).
 * QEMU: -netdev user,id=net0 -device virtio-net-pci,netdev=net0
 */
#include <linux/module.h>
#include <linux/pci.h>
#include <linux/io.h>
#include <linux/interrupt.h>
#include <linux/dma-mapping.h>
#include <linux/string.h>
#include <linux/errno.h>
#include <linux/jiffies.h>
#include <linux/gfp.h>
#include <linux/munux_net.h>

#define VIRTIO_PCI_CAP_COMMON_CFG	1
#define VIRTIO_PCI_CAP_NOTIFY_CFG	2
#define VIRTIO_PCI_CAP_ISR_CFG		3
#define VIRTIO_PCI_CAP_DEVICE_CFG	4
#define PCI_CAP_ID_VNDR			0x09
#define VIRTIO_F_VERSION_1		32
#define VIRTIO_NET_F_MAC		5
#define VIRTIO_NET_F_STATUS		16
#define VIRTIO_NET_S_LINK_UP		1
#define VIRTIO_CONFIG_S_ACKNOWLEDGE	1
#define VIRTIO_CONFIG_S_DRIVER		2
#define VIRTIO_CONFIG_S_DRIVER_OK	4
#define VIRTIO_CONFIG_S_FEATURES_OK	8
#define VIRTIO_CONFIG_S_FAILED		128
#define VRING_DESC_F_NEXT		1
#define VRING_DESC_F_WRITE		2
#define QSZ_MAX				32
#define NRX				8
#define RX_BUFSZ			4096
#define VIRTIO_NET_F_MRG_RXBUF		15
#define NET_HDR_LEN			12

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

struct vq {
	volatile struct vring_desc *desc;
	volatile struct vring_avail *avail;
	volatile struct vring_used *used;
	u64 desc_dma, avail_dma, used_dma;
	u16 qsz;
	u16 last_used;
	u16 noff;
	void *desc_cpu, *avail_cpu, *used_cpu;
};

static struct pci_dev *vn_pdev;
static void __iomem *vn_bars[6];
static volatile struct virtio_pci_common_cfg *vn_cfg;
static volatile u8 *vn_notify_base;
static u32 vn_notify_mult;
static volatile u8 *vn_isr;
static volatile u8 *vn_devcfg;
static struct vq rxq, txq;
static void *rx_cpu[NRX];
static u64 rx_dma[NRX];
static void *tx_cpu;
static u64 tx_dma;
static u8 vn_mac[6];
static int vn_ready;

static void *map_cap(struct pci_dev *pdev, u8 pos)
{
	u8 bar;
	u32 off, len;

	pci_read_config_byte(pdev, pos + 4, &bar);
	pci_read_config_dword(pdev, pos + 8, &off);
	pci_read_config_dword(pdev, pos + 12, &len);
	if (bar > 5 || !len)
		return NULL;
	if (!vn_bars[bar])
		vn_bars[bar] = pci_iomap(pdev, bar, 0x10000);
	if (!vn_bars[bar])
		return NULL;
	return (u8 *)vn_bars[bar] + off;
}

static int find_caps(struct pci_dev *pdev)
{
	u8 pos, next;

	pos = pci_find_capability(pdev, PCI_CAP_ID_VNDR);
	while (pos) {
		u8 type;

		pci_read_config_byte(pdev, pos + 3, &type);
		if (type == VIRTIO_PCI_CAP_COMMON_CFG)
			vn_cfg = map_cap(pdev, pos);
		else if (type == VIRTIO_PCI_CAP_NOTIFY_CFG) {
			vn_notify_base = map_cap(pdev, pos);
			pci_read_config_dword(pdev, pos + 16, &vn_notify_mult);
			if (vn_notify_mult == 0)
				vn_notify_mult = 1;
		} else if (type == VIRTIO_PCI_CAP_ISR_CFG)
			vn_isr = map_cap(pdev, pos);
		else if (type == VIRTIO_PCI_CAP_DEVICE_CFG)
			vn_devcfg = map_cap(pdev, pos);
		pci_read_config_byte(pdev, pos + 1, &next);
		pos = next & 0xFC;
		if (pos < 0x40)
			break;
	}
	return (vn_cfg && vn_notify_base) ? 0 : -ENODEV;
}

static void set_status(u8 s)
{
	writeb(s, (void *)&vn_cfg->device_status);
	wmb();
}

static int setup_vq(u16 sel, struct vq *q)
{
	u16 qsz;

	writew(sel, (void *)&vn_cfg->queue_select);
	writew(0xFFFF, (void *)&vn_cfg->msix_config);
	writew(0xFFFF, (void *)&vn_cfg->queue_msix_vector);
	qsz = readw((void *)&vn_cfg->queue_size);
	if (qsz == 0)
		return -ENODEV;
	if (qsz > QSZ_MAX)
		qsz = QSZ_MAX;
	q->qsz = qsz;
	writew(qsz, (void *)&vn_cfg->queue_size);
	q->desc_cpu = dma_alloc_coherent(4096, &q->desc_dma, GFP_KERNEL);
	q->avail_cpu = dma_alloc_coherent(4096, &q->avail_dma, GFP_KERNEL);
	q->used_cpu = dma_alloc_coherent(4096, &q->used_dma, GFP_KERNEL);
	if (!q->desc_cpu || !q->avail_cpu || !q->used_cpu)
		return -ENOMEM;
	memset(q->desc_cpu, 0, 4096);
	memset(q->avail_cpu, 0, 4096);
	memset(q->used_cpu, 0, 4096);
	q->desc = q->desc_cpu;
	q->avail = q->avail_cpu;
	q->used = q->used_cpu;
	writel((u32)q->desc_dma, (void *)&vn_cfg->queue_desc_lo);
	writel((u32)(q->desc_dma >> 32), (void *)&vn_cfg->queue_desc_hi);
	writel((u32)q->avail_dma, (void *)&vn_cfg->queue_driver_lo);
	writel((u32)(q->avail_dma >> 32), (void *)&vn_cfg->queue_driver_hi);
	writel((u32)q->used_dma, (void *)&vn_cfg->queue_device_lo);
	writel((u32)(q->used_dma >> 32), (void *)&vn_cfg->queue_device_hi);
	wmb();
	writew(1, (void *)&vn_cfg->queue_enable);
	q->noff = readw((void *)&vn_cfg->queue_notify_off);
	q->last_used = 0;
	return 0;
}

static void vq_notify(struct vq *q, u16 qidx)
{
	volatile u8 *p = vn_notify_base + (unsigned long)q->noff * vn_notify_mult;

	wmb();
	writew(qidx, (void *)p);
}

static int negotiate(void)
{
	u32 lo, hi, want_lo;

	set_status(0);
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE);
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE | VIRTIO_CONFIG_S_DRIVER);
	writel(1, (void *)&vn_cfg->device_feature_select);
	hi = readl((void *)&vn_cfg->device_feature);
	if ((hi & 1) == 0) {
		set_status(VIRTIO_CONFIG_S_FAILED);
		return -ENODEV;
	}
	writel(0, (void *)&vn_cfg->device_feature_select);
	lo = readl((void *)&vn_cfg->device_feature);
	want_lo = lo & ((1u << VIRTIO_NET_F_MAC) | (1u << VIRTIO_NET_F_STATUS) |
			(1u << VIRTIO_NET_F_MRG_RXBUF));
	writel(0, (void *)&vn_cfg->driver_feature_select);
	writel(want_lo, (void *)&vn_cfg->driver_feature);
	writel(1, (void *)&vn_cfg->driver_feature_select);
	writel(1, (void *)&vn_cfg->driver_feature);
	wmb();
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE | VIRTIO_CONFIG_S_DRIVER |
		   VIRTIO_CONFIG_S_FEATURES_OK);
	if (!(readb((void *)&vn_cfg->device_status) & VIRTIO_CONFIG_S_FEATURES_OK)) {
		set_status(VIRTIO_CONFIG_S_FAILED);
		return -ENODEV;
	}
	if (vn_devcfg) {
		int i;

		for (i = 0; i < 6; i++)
			vn_mac[i] = readb((void *)(vn_devcfg + i));
	}
	if (!vn_devcfg || (vn_mac[0] | vn_mac[1] | vn_mac[2] | vn_mac[3] |
			    vn_mac[4] | vn_mac[5]) == 0) {
		vn_mac[0] = 0x52;
		vn_mac[1] = 0x54;
		vn_mac[2] = 0x00;
		vn_mac[3] = 0x12;
		vn_mac[4] = 0x34;
		vn_mac[5] = 0x56;
	}
	return 0;
}

static void rx_post(u16 id)
{
	struct vq *q = &rxq;

	q->desc[id].addr = rx_dma[id];
	q->desc[id].len = RX_BUFSZ;
	q->desc[id].flags = VRING_DESC_F_WRITE;
	q->desc[id].next = 0;
	q->avail->ring[q->avail->idx % q->qsz] = id;
	wmb();
	q->avail->idx++;
}

static int vn_in_poll;

static void vn_poll(void)
{
	struct vq *q = &rxq;

	if (!vn_ready || !q->used || vn_in_poll)
		return;
	vn_in_poll = 1;
	for (;;) {
		u16 id;
		u32 len;

		rmb();
		if (q->used->idx == q->last_used)
			break;
		id = (u16)q->used->ring[q->last_used % q->qsz].id;
		len = q->used->ring[q->last_used % q->qsz].len;
		q->last_used++;
		if (id < NRX && len > NET_HDR_LEN)
			munux_netif_rx((u8 *)rx_cpu[id] + NET_HDR_LEN,
				       len - NET_HDR_LEN);
		if (id < NRX)
			rx_post(id);
	}
	vn_in_poll = 0;
}

static int vn_xmit(const void *frame, unsigned int len)
{
	struct vq *q = &txq;
	u8 *p;
	u16 aidx;
	unsigned long t0;

	if (!vn_ready || !frame || len < 14 || len > 1514)
		return -EINVAL;
	p = tx_cpu;
	memset(p, 0, NET_HDR_LEN);
	memcpy(p + NET_HDR_LEN, frame, len);
	if (len < 60) {
		memset(p + NET_HDR_LEN + len, 0, 60 - len);
		len = 60;
	}
	q->desc[0].addr = tx_dma;
	q->desc[0].len = NET_HDR_LEN + len;
	q->desc[0].flags = 0;
	q->desc[0].next = 0;
	aidx = q->avail->idx;
	q->avail->ring[aidx % q->qsz] = 0;
	wmb();
	q->avail->idx = aidx + 1;
	vq_notify(q, 1);
	t0 = jiffies;
	while (q->used->idx == q->last_used) {
		if (jiffies - t0 > HZ) {
			printk(KERN_ERR "virtio_net: tx timeout\n");
			return -EIO;
		}
		__asm__ volatile("pause");
	}
	q->last_used = q->used->idx;
	vq_notify(&rxq, 0);
	return 0;
}

static irqreturn_t vn_irq(int irq, void *dev)
{
	(void)irq;
	(void)dev;
	if (vn_isr)
		(void)readb((void *)vn_isr);
	vn_poll();
	return IRQ_HANDLED;
}

static void free_vq(struct vq *q)
{
	if (q->desc_cpu)
		dma_free_coherent(4096, q->desc_cpu, q->desc_dma);
	if (q->avail_cpu)
		dma_free_coherent(4096, q->avail_cpu, q->avail_dma);
	if (q->used_cpu)
		dma_free_coherent(4096, q->used_cpu, q->used_dma);
	memset(q, 0, sizeof(*q));
}

static void vn_teardown(struct pci_dev *pdev)
{
	int i;

	if (vn_ready) {
		munux_unregister_nic();
		vn_ready = 0;
	}
	if (pdev && pdev->irq)
		free_irq(pdev->irq, pdev);
	for (i = 0; i < NRX; i++) {
		if (rx_cpu[i]) {
			dma_free_coherent(4096, rx_cpu[i], rx_dma[i]);
			rx_cpu[i] = NULL;
		}
	}
	if (tx_cpu) {
		dma_free_coherent(4096, tx_cpu, tx_dma);
		tx_cpu = NULL;
	}
	free_vq(&rxq);
	free_vq(&txq);
	for (i = 0; i < 6; i++) {
		if (vn_bars[i]) {
			pci_iounmap(pdev, vn_bars[i]);
			vn_bars[i] = NULL;
		}
	}
	vn_cfg = NULL;
	vn_notify_base = NULL;
	vn_isr = NULL;
	vn_devcfg = NULL;
}

static int vn_probe(struct pci_dev *pdev, const struct pci_device_id *id)
{
	u16 nq;
	int i;

	(void)id;
	if (vn_ready)
		return -EBUSY;
	if (pci_enable_device(pdev))
		return -EIO;
	if (find_caps(pdev)) {
		printk(KERN_ERR "virtio_net: not modern virtio-pci\n");
		return -ENODEV;
	}
	if (negotiate()) {
		printk(KERN_ERR "virtio_net: negotiate failed\n");
		return -ENODEV;
	}
	nq = readw((void *)&vn_cfg->num_queues);
	if (nq < 2) {
		printk(KERN_ERR "virtio_net: need 2 queues\n");
		return -ENODEV;
	}
	/* Spec is 0=RX 1=TX; QEMU may still complete TX on 1 with no wire I/O.
	 * Keep spec order but kick RX with index 0 after every TX. */
	if (setup_vq(0, &rxq) || setup_vq(1, &txq)) {
		printk(KERN_ERR "virtio_net: queue setup failed\n");
		vn_teardown(pdev);
		return -ENOMEM;
	}
	writew(0, (void *)&vn_cfg->queue_select);
	writew(1, (void *)&vn_cfg->queue_enable);
	writew(1, (void *)&vn_cfg->queue_select);
	writew(1, (void *)&vn_cfg->queue_enable);
	for (i = 0; i < NRX; i++) {
		rx_cpu[i] = dma_alloc_coherent(4096, &rx_dma[i], GFP_KERNEL);
		if (!rx_cpu[i]) {
			vn_teardown(pdev);
			return -ENOMEM;
		}
		memset(rx_cpu[i], 0, 4096);
	}
	tx_cpu = dma_alloc_coherent(4096, &tx_dma, GFP_KERNEL);
	if (!tx_cpu) {
		vn_teardown(pdev);
		return -ENOMEM;
	}
	set_status(VIRTIO_CONFIG_S_ACKNOWLEDGE | VIRTIO_CONFIG_S_DRIVER |
		   VIRTIO_CONFIG_S_FEATURES_OK | VIRTIO_CONFIG_S_DRIVER_OK);
	for (i = 0; i < NRX; i++)
		rx_post((u16)i);
	wmb();
	vq_notify(&rxq, 0);
	if (pdev->irq)
		request_irq(pdev->irq, vn_irq, IRQF_SHARED, "virtio_net", pdev);
	if (munux_register_nic("eth0", vn_mac, vn_xmit, vn_poll)) {
		vn_teardown(pdev);
		return -EIO;
	}
	vn_pdev = pdev;
	vn_ready = 1;
	printk(KERN_INFO "virtio_net: nic ready\n");
	return 0;
}

/* silence unused if probe prints below */

static void vn_remove(struct pci_dev *pdev)
{
	vn_teardown(pdev);
	vn_pdev = NULL;
	printk(KERN_INFO "virtio_net: removed\n");
}

static const struct pci_device_id vn_ids[] = {
	{ PCI_DEVICE(0x1af4, 0x1000) },
	{ PCI_DEVICE(0x1af4, 0x1041) },
	{ 0, }
};

static struct pci_driver vn_drv = {
	.name = "virtio_net",
	.id_table = vn_ids,
	.probe = vn_probe,
	.remove = vn_remove,
};

static int vn_init(void)
{
	int ret = pci_register_driver(&vn_drv);

	if (ret)
		return ret;
	if (!vn_ready) {
		pci_unregister_driver(&vn_drv);
		printk(KERN_ERR "virtio_net: no virtio-net PCI device\n");
		return -ENODEV;
	}
	ret = munux_icmp_ping(0x0A000202, 3 * HZ);
	if (ret) {
		printk(KERN_ERR "virtio_net: ping 10.0.2.2 FAIL\n");
		pci_unregister_driver(&vn_drv);
		return ret;
	}
	printk(KERN_INFO "virtio_net: ping 10.0.2.2 PASS\n");
	return 0;
}

static void vn_exit(void)
{
	pci_unregister_driver(&vn_drv);
}

module_init(vn_init);
module_exit(vn_exit);
MODULE_LICENSE("GPL");
MODULE_INFO(name, "virtio_net");
MODULE_DESCRIPTION("munux linuxkpi virtio-net");
MODULE_DEVICE_TABLE(pci, vn_ids);
