; U6: fork + execve("/bin/hello") + wait4
bits 64
section .text
global _start
_start:
	mov rax, 57			; fork
	syscall
	test rax, rax
	js .fail
	jz .child

	; parent wait4
	mov rax, 61
	mov rdi, -1
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	test rax, rax
	js .fail

	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_ok]
	mov rdx, msg_ok_len
	syscall

	mov rax, 60
	xor rdi, rdi
	syscall
	jmp .hang

.child:
	; execve("/bin/hello", NULL, NULL)
	mov rax, 59
	lea rdi, [rel path_hello]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	; if returned, failed
	jmp .fail

.fail:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

.hang:
	jmp .hang

section .data
status:	dd 0
path_hello:	db "/bin/hello", 0
msg_ok:	db "exectest parent ok", 10
msg_ok_len equ $ - msg_ok
msg_fail:	db "exectest fail", 10
msg_fail_len equ $ - msg_fail
