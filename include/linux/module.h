/* munux linuxkpi — module macros (no vermagic / THIS_MODULE yet). */
#ifndef _LINUX_MODULE_H
#define _LINUX_MODULE_H

#include <linux/init.h>
#include <linux/printk.h>

struct module;

#define THIS_MODULE ((struct module *)0)

#define MODULE_INFO(tag, info)                                         \
    static const char __modinfo_##tag[]                                \
        __attribute__((section(".modinfo"), used, aligned(1))) =       \
            #tag "=" info

#define MODULE_LICENSE(s)      MODULE_INFO(license, s)
#define MODULE_AUTHOR(s)       MODULE_INFO(author, s)
#define MODULE_DESCRIPTION(s)  MODULE_INFO(description, s)

#endif /* _LINUX_MODULE_H */
