; brk smoke — Linux syscall 12
; Query break, grow by 4K, write/read pattern, print OK
bits 64
section .text
global _start

%define SYS_BRK		12
%define SYS_WRITE	1
%define SYS_EXIT	60

_start:
	; brk(0) → current break (Linux returns start_brk when 0 is below it)
	mov rax, SYS_BRK
	xor rdi, rdi
	syscall
	mov r12, rax			; old break
	test r12, r12
	jz .fail

	; grow by 4096
	lea rdi, [r12 + 4096]
	mov rax, SYS_BRK
	syscall
	cmp rax, r12
	jbe .fail			; must strictly grow
	mov r13, rax			; new break

	; write pattern at old break
	mov byte [r12], 0xA5
	mov byte [r12 + 1], 0x5A
	mov dword [r12 + 4], 0x424B524B	; 'BRKB'

	; read back
	cmp byte [r12], 0xA5
	jne .fail
	cmp byte [r12 + 1], 0x5A
	jne .fail
	cmp dword [r12 + 4], 0x424B524B
	jne .fail

	; brk(0) again must return new break
	mov rax, SYS_BRK
	xor rdi, rdi
	syscall
	cmp rax, r13
	jne .fail

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_ok]
	mov rdx, msg_ok_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

.fail:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

section .rodata
msg_ok:		db "brk: grow/write OK", 10
msg_ok_len equ $ - msg_ok
msg_fail:	db "brk: FAILED", 10
msg_fail_len equ $ - msg_fail
