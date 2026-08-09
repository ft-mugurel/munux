/* linuxkpi L3: shared IRQ0 (PIT) + completion.
 * Installed as /lib/modules/irqtest.ko
 */
#include <linux/module.h>
#include <linux/interrupt.h>
#include <linux/completion.h>
#include <linux/jiffies.h>
#include <linux/errno.h>

static struct completion saw_irq;

static irqreturn_t irqtest_handler(int irq, void *dev)
{
	(void)irq;
	(void)dev;
	complete(&saw_irq);
	return IRQ_HANDLED;
}

static int irqtest_init(void)
{
	int ret;

	init_completion(&saw_irq);
	ret = request_irq(0, irqtest_handler, IRQF_SHARED, "irqtest", &saw_irq);
	if (ret) {
		printk(KERN_ERR "irqtest: request_irq failed\n");
		return ret;
	}
	if (!wait_for_completion_timeout(&saw_irq, HZ)) {
		free_irq(0, &saw_irq);
		printk(KERN_ERR "irqtest: timeout waiting for IRQ0\n");
		return -ETIMEDOUT;
	}
	printk(KERN_INFO "irqtest: got IRQ0 (timer) PASS\n");
	return 0;
}

static void irqtest_exit(void)
{
	free_irq(0, &saw_irq);
	printk(KERN_INFO "irqtest: unloaded\n");
}

module_init(irqtest_init);
module_exit(irqtest_exit);
MODULE_LICENSE("GPL");
MODULE_INFO(name, "irqtest");
MODULE_DESCRIPTION("munux linuxkpi IRQ0 shared smoke");
