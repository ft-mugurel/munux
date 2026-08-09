/* munux linuxkpi — module_init / module_exit (classic init_module names). */
#ifndef _LINUX_INIT_H
#define _LINUX_INIT_H

#define __init
#define __exit
#define __initdata
#define __exitdata

#define module_init(fn) \
    int init_module(void) { return (fn)(); }

#define module_exit(fn) \
    void cleanup_module(void) { (fn)(); }

#endif /* _LINUX_INIT_H */
