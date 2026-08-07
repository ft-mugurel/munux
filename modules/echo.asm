; munux echo chardev module — MNX1 (nasm -f bin)
;
; Registers /dev/echo. write() stores a small buffer; read() returns it.
; Relocs: printk, register_chrdev, unregister_chrdev.

bits 64
default rel

ECHO_CAP equ 128

    dd 0x31584E4D
name_field:
    db 'echo', 0
    times 28 - ($ - name_field) db 0
    dd code_end - code_start
    dd init - code_start
    dd exit_fn - code_start
    dd 4                          ; n_relocs

code_start:

init:
    lea     rdi, [rel msg_init]
    db 0x48, 0xB8
reloc_printk_init:
    dq 0
    call    rax

    lea     rdi, [rel devname]
    lea     rsi, [rel echo_read]
    lea     rdx, [rel echo_write]
    xor     ecx, ecx              ; no release hook
    db 0x48, 0xB8
reloc_reg:
    dq 0
    call    rax
    test    eax, eax
    jnz     .fail
    xor     eax, eax
    ret
.fail:
    mov     eax, 1
    ret

exit_fn:
    lea     rdi, [rel devname]
    db 0x48, 0xB8
reloc_unreg:
    dq 0
    call    rax

    lea     rdi, [rel msg_exit]
    db 0x48, 0xB8
reloc_printk_exit:
    dq 0
    call    rax
    xor     eax, eax
    ret

; long echo_write(char *buf, unsigned long len)
echo_write:
    push    rbx
    mov     rbx, rsi              ; requested len
    cmp     rbx, ECHO_CAP
    jbe     .cap
    mov     rbx, ECHO_CAP
.cap:
    test    rbx, rbx
    jz      .done
    mov     rsi, rdi              ; src = caller buf
    lea     rdi, [rel echo_buf]
    mov     rcx, rbx
    rep     movsb
.done:
    mov     dword [rel echo_len], ebx
    mov     rax, rbx
    pop     rbx
    ret

; long echo_read(char *buf, unsigned long len)
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
    ; rdi already dest
    rep     movsb
    mov     rax, r8
    ret
.empty:
    xor     eax, eax
    ret

devname:
    db 'echo', 0
msg_init:
    db 'echo: module loaded (/dev/echo)', 0
msg_exit:
    db 'echo: module unloaded', 0

echo_len:
    dd 0
echo_buf:
    times ECHO_CAP db 0

code_end:

    dd (reloc_printk_init - code_start)
s0: db 'munux_printk', 0
    times 32 - ($ - s0) db 0

    dd (reloc_reg - code_start)
s1: db 'munux_register_chrdev', 0
    times 32 - ($ - s1) db 0

    dd (reloc_unreg - code_start)
s2: db 'munux_unregister_chrdev', 0
    times 32 - ($ - s2) db 0

    dd (reloc_printk_exit - code_start)
s3: db 'munux_printk', 0
    times 32 - ($ - s3) db 0
