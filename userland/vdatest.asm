; vdatest — read /dev/vda first 9 bytes, expect "VIRTIOBLK"
bits 64
section .text
global _start

_start:
	mov rax, 2
	lea rdi, [rel path]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail_open
	mov r12, rax

	mov rax, 0
	mov rdi, r12
	lea rsi, [rel rbuf]
	mov rdx, 9
	syscall
	cmp rax, 9
	jne .fail_io

	lea rsi, [rel magic]
	lea rdi, [rel rbuf]
	mov rcx, 9
	repe cmpsb
	jne .fail_io

	mov rax, 3
	mov rdi, r12
	syscall
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_pass]
	mov rdx, msg_pass_len
	syscall
	mov rax, 60
	xor rdi, rdi
	syscall

.fail_open:
	lea rsi, [rel msg_open]
	mov rdx, msg_open_len
	jmp .err
.fail_io:
	mov rax, 3
	mov rdi, r12
	syscall
	lea rsi, [rel msg_io]
	mov rdx, msg_io_len
.err:
	mov rax, 1
	mov rdi, 2
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

section .rodata
path:		db "/dev/vda", 0
magic:		db "VIRTIOBLK"
msg_pass:	db "vdatest: PASS", 10
msg_pass_len	equ $ - msg_pass
msg_open:	db "vdatest: open /dev/vda failed", 10
msg_open_len	equ $ - msg_open
msg_io:		db "vdatest: read mismatch", 10
msg_io_len	equ $ - msg_io

section .bss
align 16
rbuf:	resb 16
