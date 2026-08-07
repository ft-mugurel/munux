; munux hello — ELF64 ET_REL (.ko)
; nasm -f elf64 -o hello.ko hello.ko.asm
;
; Relocs: R_X86_64_PC32 to munux_printk (loader emits abs64 trampoline).

bits 64
default rel

section .text
global init_module
global cleanup_module
extern munux_printk

init_module:
    lea     rdi, [rel msg_init]
    call    munux_printk
    xor     eax, eax
    ret

cleanup_module:
    lea     rdi, [rel msg_exit]
    call    munux_printk
    xor     eax, eax
    ret

section .rodata
msg_init:
    db 'hello: module loaded (elf)', 0
msg_exit:
    db 'hello: module unloaded (elf)', 0

section .modinfo
    db 'name=hello', 0
