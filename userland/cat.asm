; cat hello.txt via Linux open/read/write/close/exit
; open=2 read=0 write=1 close=3 exit=60
bits 64
section .text
global _start
_start:
	; open("hello.txt", O_RDONLY=0)
	mov rax, 2
	lea rdi, [rel path]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail
	mov r12, rax			; fd

.read:
	mov rax, 0
	mov rdi, r12
	lea rsi, [rel buf]
	mov rdx, 128
	syscall
	cmp rax, -4095
	jae .fail_close
	test rax, rax
	jz .done
	mov r13, rax			; n

	mov rax, 1
	mov rdi, 1
	lea rsi, [rel buf]
	mov rdx, r13
	syscall
	jmp .read

.done:
	mov rax, 3
	mov rdi, r12
	syscall
	mov rax, 60
	xor rdi, rdi
	syscall

.fail_close:
	mov rax, 3
	mov rdi, r12
	syscall
.fail:
	mov rax, 60
	mov rdi, 1
	syscall

section .rodata
path:	db "hello.txt", 0

section .bss
align 16
buf:	resb 128
