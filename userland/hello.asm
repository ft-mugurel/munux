; Freestanding x86_64 user program for munux
; Linux x86_64 syscall numbers: write=1 exit=60
bits 64
section .text
global _start
_start:
	mov rax, 1			; SYS_write
	mov rdi, 1			; stdout
	lea rsi, [rel msg]
	mov rdx, msg_len
	syscall

	mov rax, 60			; SYS_exit
	xor rdi, rdi
	syscall

.hang:
	jmp .hang

section .rodata
msg:	db "Hello from ELF64 userland!", 10
msg_end:
msg_len equ msg_end - msg
