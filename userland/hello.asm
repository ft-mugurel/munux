; Tiny freestanding i386 user program: write(1, msg, n); exit(0);
; Syscall ABI: int 0x80 — EAX=num, EBX,ECX,EDX (KFS)
; SYS_EXIT=0  SYS_WRITE=1
bits 32
section .text
global _start
_start:
	mov eax, 1			; SYS_WRITE
	mov ebx, 1			; stdout
	mov ecx, msg
	mov edx, msg_len
	int 0x80

	mov eax, 0			; SYS_EXIT
	xor ebx, ebx
	int 0x80

.hang:
	jmp .hang

section .rodata
msg:	db "Hello from ELF userland!", 10
msg_end:
msg_len equ msg_end - msg
