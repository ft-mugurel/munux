/* linuxkpi L2: Linux misc char device → /dev/echo
 * Installed as /lib/modules/echo_c.ko; module name is "echo" so echotest works.
 * Do not load together with NASM echo.ko (same /dev/echo).
 */
#include <linux/module.h>
#include <linux/fs.h>
#include <linux/miscdevice.h>
#include <linux/uaccess.h>
#include <linux/errno.h>

#define ECHO_CAP 128

static char echo_buf[ECHO_CAP];
static unsigned int echo_len;

static ssize_t echo_read(struct file *f, char __user *buf, size_t len, loff_t *ppos)
{
	size_t n = echo_len;

	(void)f;
	(void)ppos;
	if (n > len)
		n = len;
	if (n && copy_to_user(buf, echo_buf, n))
		return -EFAULT;
	return (ssize_t)n;
}

static ssize_t echo_write(struct file *f, const char __user *buf, size_t len, loff_t *ppos)
{
	size_t n = len;

	(void)f;
	(void)ppos;
	if (n > ECHO_CAP)
		n = ECHO_CAP;
	if (n && copy_from_user(echo_buf, buf, n))
		return -EFAULT;
	echo_len = (unsigned int)n;
	return (ssize_t)n;
}

static const struct file_operations echo_fops = {
	.owner = THIS_MODULE,
	.read = echo_read,
	.write = echo_write,
};

static struct miscdevice echo_dev = {
	.minor = MISC_DYNAMIC_MINOR,
	.name = "echo",
	.fops = &echo_fops,
};

static int echo_init(void)
{
	int ret = misc_register(&echo_dev);

	if (ret)
		return ret;
	printk(KERN_INFO "echo_c: module loaded (/dev/echo)\n");
	return 0;
}

static void echo_exit(void)
{
	misc_deregister(&echo_dev);
	printk(KERN_INFO "echo_c: module unloaded\n");
}

module_init(echo_init);
module_exit(echo_exit);
MODULE_LICENSE("GPL");
MODULE_INFO(name, "echo");
MODULE_DESCRIPTION("munux linuxkpi echo chardev");
