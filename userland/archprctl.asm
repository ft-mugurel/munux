; archprctl smoke — SET_FS / GET_FS (Linux syscall 158)
; ARCH_SET_FS=0x1002 ARCH_GET_FS=0x1003
bits 64
section .text
global _start

%define SYS_ARCH_PRCTL	158
%define SYS_WRITE	1
%define SYS_EXIT	60
%define ARCH_SET_FS	0x1002
%define ARCH_GET_FS	0x1003

_start:
	; SET_FS → address of tls_blob
	mov rax, SYS_ARCH_PRCTL
	mov rdi, ARCH_SET_FS
	lea rsi, [rel tls_blob]
	syscall
	cmp rax, -4095
	jae .fail

	; GET_FS → out_val
	mov rax, SYS_ARCH_PRCTL
	mov rdi, ARCH_GET_FS
	lea rsi, [rel out_val]
	syscall
	cmp rax, -4095
	jae .fail

	; compare out_val with &tls_blob
	lea rax, [rel tls_blob]
	mov rbx, [rel out_val]
	cmp rax, rbx
	jne .mismatch

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_ok]
	mov rdx, msg_ok_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

.mismatch:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_bad]
	mov rdx, msg_bad_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 2
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
msg_ok:		db "arch_prctl: FS set/get OK", 10
msg_ok_len equ $ - msg_ok
msg_bad:	db "arch_prctl: FS mismatch", 10
msg_bad_len equ $ - msg_bad
msg_fail:	db "arch_prctl: syscall failed", 10
msg_fail_len equ $ - msg_fail

section .bss
align 16
tls_blob:	resb 64
out_val:	resq 1
