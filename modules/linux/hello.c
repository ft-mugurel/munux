/* linuxkpi L1 smoke: gcc ET_REL, printk + module_init.
 * Installed as /lib/modules/hello_c.ko (does not replace NASM hello.ko).
 */
#include <linux/module.h>
#include <linux/printk.h>

static int hello_init(void)
{
	printk(KERN_INFO "hello_c: linuxkpi module loaded\n");
	return 0;
}

static void hello_exit(void)
{
	printk(KERN_INFO "hello_c: linuxkpi module unloaded\n");
}

module_init(hello_init);
module_exit(hello_exit);
MODULE_LICENSE("GPL");
MODULE_INFO(name, "hello_c");
MODULE_DESCRIPTION("munux linuxkpi hello");
