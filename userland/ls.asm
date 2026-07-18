; ls "." via open + getdents64 + write
; open=2 getdents64=217 write=1 close=3 exit=60
bits 64
section .text
global _start
_start:
	mov rax, 2
	lea rdi, [rel dot]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail
	mov r12, rax

.loop:
	mov rax, 217
	mov rdi, r12
	lea rsi, [rel buf]
	mov rdx, 512
	syscall
	cmp rax, -4095
	jae .fail_close
	test rax, rax
	jz .done
	mov r13, rax			; bytes
	xor r14, r14			; pos

.next_ent:
	cmp r14, r13
	jae .loop
	lea rbx, [rel buf]
	add rbx, r14
	movzx r15, word [rbx + 16]	; d_reclen (save across syscalls!)
	test r15, r15
	jz .done
	lea rsi, [rbx + 19]		; d_name
	xor rdx, rdx
.strlen:
	cmp byte [rsi + rdx], 0
	je .print
	inc rdx
	jmp .strlen
.print:
	mov rax, 1
	mov rdi, 1
	syscall
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel nl]
	mov rdx, 1
	syscall
	add r14, r15
	jmp .next_ent

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
dot:	db ".", 0
nl:	db 10

section .bss
align 16
buf:	resb 512
