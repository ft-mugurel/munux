; mmap/munmap smoke — Linux syscalls 9 / 11
; MAP_PRIVATE|MAP_ANONYMOUS, write pattern, munmap, exit 0
bits 64
section .text
global _start

%define SYS_MMAP	9
%define SYS_MUNMAP	11
%define SYS_WRITE	1
%define SYS_EXIT	60

%define PROT_READ	1
%define PROT_WRITE	2
%define MAP_PRIVATE	2
%define MAP_ANONYMOUS	0x20

_start:
	; mmap(NULL, 4096, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)
	mov rax, SYS_MMAP
	xor rdi, rdi			; addr
	mov rsi, 4096			; length
	mov rdx, PROT_READ | PROT_WRITE
	mov r10, MAP_PRIVATE | MAP_ANONYMOUS
	mov r8, -1			; fd
	xor r9, r9			; offset
	syscall
	cmp rax, -4095
	jae .fail
	mov r12, rax			; mapped VA

	; write pattern
	mov byte [r12], 0x4D		; 'M'
	mov byte [r12 + 1], 0x4D	; 'M'
	mov dword [r12 + 4], 0x50414D4D	; 'MMAP' le

	cmp byte [r12], 0x4D
	jne .fail
	cmp dword [r12 + 4], 0x50414D4D
	jne .fail

	; munmap
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
	cmp rax, -4095
	jae .fail

	; second map should succeed
	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 8192
	mov rdx, PROT_READ | PROT_WRITE
	mov r10, MAP_PRIVATE | MAP_ANONYMOUS
	mov r8, -1
	xor r9, r9
	syscall
	cmp rax, -4095
	jae .fail
	mov r12, rax
	mov rax, 0x1122334455667788
	mov [r12], rax
	cmp [r12], rax
	jne .fail

	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 8192
	syscall
	cmp rax, -4095
	jae .fail

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_ok]
	mov rdx, msg_ok_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

.fail:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

section .rodata
msg_ok:		db "mmap: anon map/write/unmap OK", 10
msg_ok_len equ $ - msg_ok
msg_fail:	db "mmap: FAILED", 10
msg_fail_len equ $ - msg_fail
