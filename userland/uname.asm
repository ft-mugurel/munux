; uname — print Linux utsname fields via syscall 63
; Linux x86_64: uname=63 write=1 exit=60
bits 64
section .text
global _start

%define UTS_LEN 65

_start:
	mov rax, 63
	lea rdi, [rel uts]
	syscall
	cmp rax, -4095
	jae .fail

	lea r12, [rel uts]
	lea rdi, [rel lbl_sys]
	mov rsi, r12
	call print_field

	lea rdi, [rel lbl_node]
	lea rsi, [r12 + UTS_LEN]
	call print_field

	lea rdi, [rel lbl_rel]
	lea rsi, [r12 + UTS_LEN*2]
	call print_field

	lea rdi, [rel lbl_ver]
	lea rsi, [r12 + UTS_LEN*3]
	call print_field

	lea rdi, [rel lbl_mach]
	lea rsi, [r12 + UTS_LEN*4]
	call print_field

	mov rax, 60
	xor rdi, rdi
	syscall

.fail:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_err]
	mov rdx, msg_err_len
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

; rdi = NUL-terminated label, rsi = field value (NUL in 65-byte slot)
print_field:
	push rbx
	push r12
	mov r12, rsi			; save value ptr
	; strlen label
	xor rcx, rcx
.sl:
	cmp byte [rdi+rcx], 0
	je .sld
	inc rcx
	jmp .sl
.sld:
	mov rsi, rdi			; label
	mov rdx, rcx
	mov rax, 1
	mov rdi, 1
	syscall
	; strlen value (max UTS_LEN-1)
	xor rcx, rcx
.sv:
	cmp rcx, UTS_LEN
	jae .svd
	cmp byte [r12+rcx], 0
	je .svd
	inc rcx
	jmp .sv
.svd:
	mov rsi, r12
	mov rdx, rcx
	mov rax, 1
	mov rdi, 1
	syscall
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel nl]
	mov rdx, 1
	syscall
	pop r12
	pop rbx
	ret

section .rodata
lbl_sys:	db "sysname: ", 0
lbl_node:	db "nodename: ", 0
lbl_rel:	db "release: ", 0
lbl_ver:	db "version: ", 0
lbl_mach:	db "machine: ", 0
nl:		db 10
msg_err:	db "uname failed", 10
msg_err_len equ $ - msg_err

section .bss
align 16
uts:	resb 65*6
