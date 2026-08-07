; munux hello module — MNX1 container (nasm -f bin)
;
; Layout matches src/module/mnx.rs. Code is position-independent for RIP-relative
; string loads; kernel exports are absolute 64-bit values patched at reloc sites.

bits 64
default rel

; ---------------------------------------------------------------------------
; Header (48 bytes)
; ---------------------------------------------------------------------------
    dd 0x31584E4D                 ; magic 'MNX1'
name_field:
    db 'hello', 0
    times 28 - ($ - name_field) db 0
    dd code_end - code_start      ; code_len
    dd init - code_start          ; init_off
    dd exit_fn - code_start       ; exit_off
    dd 2                          ; n_relocs (printk in init + exit)

; ---------------------------------------------------------------------------
; Code image
; ---------------------------------------------------------------------------
code_start:

init:
    lea     rdi, [rel msg_init]
    ; Force REX.W mov r64, imm64 so loader can patch 8 bytes (NASM would
    ; otherwise optimize `mov rax, 0` to 5-byte `mov eax, 0`).
    db 0x48, 0xB8
reloc0_imm:
    dq 0                          ; patched → munux_printk
    call    rax
    xor     eax, eax
    ret

exit_fn:
    lea     rdi, [rel msg_exit]
    db 0x48, 0xB8
reloc1_imm:
    dq 0                          ; patched → munux_printk
    call    rax
    xor     eax, eax
    ret

msg_init:
    db 'hello: module loaded (mnx)', 0
msg_exit:
    db 'hello: module unloaded (mnx)', 0

code_end:

; ---------------------------------------------------------------------------
; Relocations: absolute offset of the 8-byte imm64 patch site in code
; ---------------------------------------------------------------------------
    dd (reloc0_imm - code_start)
sym0:
    db 'munux_printk', 0
    times 32 - ($ - sym0) db 0

    dd (reloc1_imm - code_start)
sym1:
    db 'munux_printk', 0
    times 32 - ($ - sym1) db 0
