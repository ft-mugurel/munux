; U6: fork + wait4 smoke test (Linux x86_64 numbers)
; child prints "child\n" and exits 42; parent waits and prints "parent\n"
bits 64
section .text
global _start
_start:
	; fork
	mov rax, 57
	syscall
	test rax, rax
	js .fail
	jz .child

	; parent: wait4(-1, &status, 0, 0)
	mov rax, 61
	mov rdi, -1
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	test rax, rax
	js .fail

	; write "parent\n"
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_parent]
	mov rdx, msg_parent_len
	syscall

	; exit 0
	mov rax, 60
	xor rdi, rdi
	syscall
	jmp .hang

.child:
	; write "child\n"
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_child]
	mov rdx, msg_child_len
	syscall

	; exit 42
	mov rax, 60
	mov rdi, 42
	syscall

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
msg_parent:	db "parent", 10
msg_parent_len equ $ - msg_parent
msg_child:	db "child", 10
msg_child_len equ $ - msg_child
msg_fail:	db "forktest fail", 10
msg_fail_len equ $ - msg_fail
