/* P10a: ET_EXEC with PT_INTERP /lib/ld-munux.so (gcc --dynamic-linker). */
    .section .interp,"a"
    .asciz "/lib/ld-munux.so"

    .section .text
    .global _start
_start:
    mov $1, %rax
    mov $1, %rdi
    lea msg(%rip), %rsi
    mov $msg_len, %rdx
    syscall
    mov $1, %rax
    mov $1, %rdi
    lea ok(%rip), %rsi
    mov $ok_len, %rdx
    syscall
    mov $60, %rax
    xor %rdi, %rdi
    syscall

    .section .rodata
msg:
    .ascii "dynlink B: main OK\n"
    .set msg_len, . - msg
ok:
    .ascii "dynlinktest: ALL PASS\n"
    .set ok_len, . - ok
