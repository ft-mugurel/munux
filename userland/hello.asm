; Freestanding x86_64 user program for munux
; Syscall ABI: rax=num, rdi/rsi/rdx — EXIT=0 WRITE=1
bits 64
section .text
global _start
_start:
	mov rax, 1			; SYS_WRITE
	mov rdi, 1			; stdout
	lea rsi, [rel msg]
	mov rdx, msg_len
	syscall

	mov rax, 0			; SYS_EXIT
	xor rdi, rdi
	syscall

.hang:
	jmp .hang

section .rodata
msg:	db "Hello from ELF64 userland!", 10
msg_end:
msg_len equ msg_end - msg
