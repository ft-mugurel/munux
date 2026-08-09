/* munux linuxkpi — compatible types (not a Linux kernel header copy). */
#ifndef _LINUX_TYPES_H
#define _LINUX_TYPES_H

#include <stddef.h>
#include <stdint.h>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;
typedef int8_t s8;
typedef int16_t s16;
typedef int32_t s32;
typedef int64_t s64;

typedef u8 __u8;
typedef u16 __u16;
typedef u32 __u32;
typedef u64 __u64;

typedef long ssize_t;
typedef long loff_t;
typedef unsigned int gfp_t;

#ifndef NULL
#define NULL ((void *)0)
#endif

#endif /* _LINUX_TYPES_H */
