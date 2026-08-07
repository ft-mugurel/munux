; rmmod <name> — unload a kernel module via delete_module (Linux #176)
; Linux x86_64: write=1 exit=60 delete_module=176
bits 64
section .text
global _start

_start:
	mov rax, [rsp]
	cmp rax, 2
	jb .usage
	mov rdi, [rsp+16]		; argv[1]
	test rdi, rdi
	jz .usage

	; delete_module(name, flags=0)
	mov rax, 176
	; rdi already name
	xor rsi, rsi
	syscall
	test rax, rax
	js .fail
	mov rax, 60
	xor rdi, rdi
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

.usage:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_usage]
	mov rdx, msg_usage_len
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

section .rodata
msg_usage:	db "usage: rmmod <name>", 10
msg_usage_len equ $ - msg_usage
msg_fail:	db "rmmod: failed", 10
msg_fail_len equ $ - msg_fail
