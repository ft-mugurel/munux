; insmod <path> — load a munux MNX1 module via finit_module (Linux #313)
; Linux x86_64: open=2 close=3 write=1 exit=60 finit_module=313
;
; Usage: insmod /lib/modules/hello.mnx
;        insmod hello.mnx   (relative path)
bits 64
section .text
global _start

_start:
	; argc on stack
	mov rax, [rsp]
	cmp rax, 2
	jb .usage
	mov rdi, [rsp+16]		; argv[1]
	test rdi, rdi
	jz .usage

	; open(path, O_RDONLY)
	mov rax, 2
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail_open
	mov r12, rax			; fd

	; finit_module(fd, uargs="", flags=0)
	mov rax, 313
	mov rdi, r12
	lea rsi, [rel empty_args]
	xor rdx, rdx
	syscall
	mov r13, rax			; save result
	; close(fd)
	mov rax, 3
	mov rdi, r12
	syscall

	test r13, r13
	js .fail_init
	; success
	mov rax, 60
	xor rdi, rdi
	syscall

.fail_init:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_init]
	mov rdx, msg_init_len
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

.fail_open:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_open]
	mov rdx, msg_open_len
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
empty_args:	db 0
msg_usage:	db "usage: insmod <path.mnx>", 10
msg_usage_len equ $ - msg_usage
msg_open:	db "insmod: cannot open module file", 10
msg_open_len equ $ - msg_open
msg_init:	db "insmod: init_module failed", 10
msg_init_len equ $ - msg_init
