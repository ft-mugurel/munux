; munux userland: read until newline (or 64 bytes), echo to stdout
; Linux x86_64: read=0 write=1 exit=60
bits 64
section .text
global _start
_start:
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel prompt]
	mov rdx, prompt_len
	syscall

	xor r12, r12
.read_loop:
	cmp r12, 64
	jae .echo
	mov rax, 0			; SYS_read
	mov rdi, 0
	lea rsi, [rel buf]
	add rsi, r12
	mov rdx, 64
	sub rdx, r12
	syscall
	; Linux: error if rax > 0xfffffffffffff000 (negative as signed)
	cmp rax, -4095
	jae .bad
	test rax, rax
	jz .read_loop
	mov rcx, rax
	lea rdi, [rel buf]
	add rdi, r12
.scan:
	cmp byte [rdi], 10
	je .got_nl
	inc rdi
	loop .scan
	add r12, rax
	jmp .read_loop
.got_nl:
	lea rax, [rel buf]
	add rax, r12
	mov rdx, rdi
	sub rdx, rax
	inc rdx
	add r12, rdx
	jmp .echo

.echo:
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel buf]
	mov rdx, r12
	syscall

	mov rax, 60
	xor rdi, rdi
	syscall

.bad:
	mov rax, 60
	mov rdi, 1
	syscall

section .rodata
prompt:	db "read> "
prompt_len equ $ - prompt

section .bss
align 16
buf:	resb 64
