; munux echo chardev — ELF64 ET_REL (.ko)
; nasm -f elf64 -o echo.ko echo.ko.asm
;
; Same fops contract as echo.asm (MNX1): register /dev/echo.

bits 64
default rel

ECHO_CAP equ 128

section .text
global init_module
global cleanup_module
extern munux_printk
extern munux_register_chrdev
extern munux_unregister_chrdev

init_module:
    lea     rdi, [rel msg_init]
    call    munux_printk

    lea     rdi, [rel devname]
    lea     rsi, [rel echo_read]
    lea     rdx, [rel echo_write]
    xor     ecx, ecx
    call    munux_register_chrdev
    test    eax, eax
    jnz     .fail
    xor     eax, eax
    ret
.fail:
    mov     eax, 1
    ret

cleanup_module:
    lea     rdi, [rel devname]
    call    munux_unregister_chrdev

    lea     rdi, [rel msg_exit]
    call    munux_printk
    xor     eax, eax
    ret

echo_write:
    push    rbx
    mov     rbx, rsi
    cmp     rbx, ECHO_CAP
    jbe     .cap
    mov     rbx, ECHO_CAP
.cap:
    test    rbx, rbx
    jz      .done
    mov     rsi, rdi
    lea     rdi, [rel echo_buf]
    mov     rcx, rbx
    rep     movsb
.done:
    mov     dword [rel echo_len], ebx
    mov     rax, rbx
    pop     rbx
    ret

echo_read:
    mov     eax, dword [rel echo_len]
    mov     rcx, rsi
    cmp     rcx, rax
    jbe     .use
    mov     rcx, rax
.use:
    test    rcx, rcx
    jz      .empty
    mov     r8, rcx
    lea     rsi, [rel echo_buf]
    rep     movsb
    mov     rax, r8
    ret
.empty:
    xor     eax, eax
    ret

section .data
echo_len:
    dd 0
echo_buf:
    times ECHO_CAP db 0

section .rodata
devname:
    db 'echo', 0
msg_init:
    db 'echo: module loaded (/dev/echo)', 0
msg_exit:
    db 'echo: module unloaded', 0

section .modinfo
    db 'name=echo', 0
