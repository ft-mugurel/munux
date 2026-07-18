; munux userland: read until newline (or 64 bytes), echo to stdout
; Syscalls: EXIT=0 WRITE=1 READ=2
bits 64
section .text
global _start
_start:
	; write(1, prompt, n)
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel prompt]
	mov rdx, prompt_len
	syscall

	xor r12, r12			; total bytes in buf
.read_loop:
	cmp r12, 64
	jae .echo
	; read(0, buf+r12, 64-r12)
	mov rax, 2
	mov rdi, 0
	lea rsi, [rel buf]
	add rsi, r12
	mov rdx, 64
	sub rdx, r12
	syscall
	cmp rax, -1
	je .bad
	test rax, rax
	jz .read_loop
	; scan new bytes for \n
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
	; r12 = offset of first new byte; include through newline
	; bytes added before \n in this chunk:
	lea rax, [rel buf]
	add rax, r12			; start of this chunk
	mov rdx, rdi
	sub rdx, rax			; index of \n within chunk
	inc rdx				; include \n
	add r12, rdx
	jmp .echo

.echo:
	; write(1, buf, r12)
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel buf]
	mov rdx, r12
	syscall

	mov rax, 0
	xor rdi, rdi
	syscall

.bad:
	mov rax, 0
	mov rdi, 1
	syscall

section .rodata
prompt:	db "read> "
prompt_len equ $ - prompt

section .bss
align 16
buf:	resb 64
