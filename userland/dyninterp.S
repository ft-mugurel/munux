/* P10a: tiny program interpreter. Loaded at 0x2000000 (see Makefile).
 * Walks auxv, checks AT_BASE / AT_ENTRY / AT_PHDR, then jumps to AT_ENTRY. */
    .section .text
    .global _start
_start:
    mov (%rsp), %rcx                 /* argc */
    lea 8(%rsp), %rsi                /* argv */
    lea 8(%rsi, %rcx, 8), %rsi       /* envp (after argv NULL) */
1:
    mov (%rsi), %rax
    add $8, %rsi
    test %rax, %rax
    jnz 1b                           /* skip envp */
    /* rsi = auxv */
    xor %r8, %r8                     /* AT_BASE */
    xor %r9, %r9                     /* AT_ENTRY */
    xor %r10, %r10                   /* AT_PHDR */
2:
    mov (%rsi), %rax
    mov 8(%rsi), %rbx
    add $16, %rsi
    test %rax, %rax
    jz 3f
    cmp $7, %rax                     /* AT_BASE */
    jne 21f
    mov %rbx, %r8
    jmp 2b
21:
    cmp $9, %rax                     /* AT_ENTRY */
    jne 22f
    mov %rbx, %r9
    jmp 2b
22:
    cmp $3, %rax                     /* AT_PHDR */
    jne 2b
    mov %rbx, %r10
    jmp 2b
3:
    test %r8, %r8
    jz fail
    test %r9, %r9
    jz fail
    test %r10, %r10
    jz fail
    mov $1, %rax
    mov $1, %rdi
    lea msga(%rip), %rsi
    mov $msga_len, %rdx
    syscall
    jmp *%r9

fail:
    mov $1, %rax
    mov $2, %rdi
    lea msgf(%rip), %rsi
    mov $msgf_len, %rdx
    syscall
    mov $60, %rax
    mov $1, %rdi
    syscall

    .section .rodata
msga:
    .ascii "dynlink A: interp auxv OK\n"
    .set msga_len, . - msga
msgf:
    .ascii "dynlink FAIL interp\n"
    .set msgf_len, . - msgf
