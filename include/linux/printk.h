/* munux linuxkpi — printk (L1: format string only, no varargs). */
#ifndef _LINUX_PRINTK_H
#define _LINUX_PRINTK_H

#define KERN_SOH        "\001"
#define KERN_EMERG      KERN_SOH "0"
#define KERN_ALERT      KERN_SOH "1"
#define KERN_CRIT       KERN_SOH "2"
#define KERN_ERR        KERN_SOH "3"
#define KERN_WARNING    KERN_SOH "4"
#define KERN_NOTICE     KERN_SOH "5"
#define KERN_INFO       KERN_SOH "6"
#define KERN_DEBUG      KERN_SOH "7"
#define KERN_CONT       KERN_SOH "c"

int printk(const char *fmt);

#define pr_info(fmt)    printk(KERN_INFO fmt)
#define pr_err(fmt)     printk(KERN_ERR fmt)
#define pr_warn(fmt)    printk(KERN_WARNING fmt)
#define pr_debug(fmt)   printk(KERN_DEBUG fmt)

#endif /* _LINUX_PRINTK_H */
