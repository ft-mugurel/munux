; P10d: clone + clone3 + CLONE_SETTLS.
;
; 1) clone(56) child FS base is the tls arg (not the parent's).
; 2) clone3(435) child keeps rdx=fn / r8=arg (glibc pthread trampoline).
;
; No CLONE_THREAD so wait4 can reap (SIGCHLD). Shared AS (CLONE_VM).
bits 64
section .text
global _start

%define SYS_WRITE		1
%define SYS_CLONE		56
%define SYS_EXIT		60
%define SYS_WAIT4		61
%define SYS_ARCH_PRCTL		158
%define SYS_CLONE3		435
%define ARCH_SET_FS		0x1002
%define SIGCHLD			17

; CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|CLONE_SETTLS|SIGCHLD
%define CLONE_FLAGS		0x00080F00 | SIGCHLD

_start:
	; ---- parent TLS ----
	mov rax, 0x1111111111111111
	mov [rel tls_parent], rax
	mov rax, SYS_ARCH_PRCTL
	mov rdi, ARCH_SET_FS
	lea rsi, [rel tls_parent]
	syscall
	cmp rax, -4095
	jae fail

	; ---- clone(56) + SETTLS ----
	mov rax, 0x2222222222222222
	mov [rel tls_child], rax
	mov rax, SYS_CLONE
	mov rdi, CLONE_FLAGS
	lea rsi, [rel stack1_top]
	xor rdx, rdx
	xor r10, r10
	lea r8, [rel tls_child]
	syscall
	test rax, rax
	js fail
	jz clone_child

	mov r12, rax
	mov rax, SYS_WAIT4
	mov rdi, r12
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	cmp dword [rel flag1], 1
	jne fail

	; ---- clone3(435) + SETTLS + rdx/r8 inherit ----
	mov rax, 0x3333333333333333
	mov [rel tls_c3], rax
	lea rax, [rel c3args]
	mov qword [rax + 0], CLONE_FLAGS
	mov qword [rax + 8], 0
	mov qword [rax + 16], 0
	mov qword [rax + 24], 0
	mov qword [rax + 32], SIGCHLD
	lea rbx, [rel stack2]
	mov [rax + 40], rbx
	mov qword [rax + 48], 4096
	lea rbx, [rel tls_c3]
	mov [rax + 56], rbx

	lea rdi, [rel c3args]
	mov rsi, 64
	lea rdx, [rel c3fn]
	mov r8, 0x42
	mov rax, SYS_CLONE3
	syscall
	test rax, rax
	js fail
	jz c3_enter

	mov r12, rax
	mov rax, SYS_WAIT4
	mov rdi, r12
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	cmp dword [rel flag3], 1
	jne fail

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_ok]
	mov rdx, msg_ok_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

clone_child:
	mov rax, [fs:0]
	mov rbx, 0x2222222222222222
	cmp rax, rbx
	jne child_bad
	mov dword [rel flag1], 1
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_child]
	mov rdx, msg_child_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

c3_enter:
	; Same trampoline as glibc clone3: rdi=arg (r8), call *rdx
	xor ebp, ebp
	mov rdi, r8
	call rdx
	mov rdi, rax
	mov rax, SYS_EXIT
	syscall

c3fn:
	cmp rdi, 0x42
	jne c3_bad
	mov rax, [fs:0]
	mov rbx, 0x3333333333333333
	cmp rax, rbx
	jne c3_bad
	mov dword [rel flag3], 1
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_c3]
	mov rdx, msg_c3_len
	syscall
	xor eax, eax
	ret

c3_bad:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_c3bad]
	mov rdx, msg_c3bad_len
	syscall
	mov eax, 1
	ret

child_bad:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_bad]
	mov rdx, msg_bad_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

fail:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

section .data
status:		dd 0
flag1:		dd 0
flag3:		dd 0
msg_ok:		db "tlsclone: ALL PASS", 10
msg_ok_len	equ $ - msg_ok
msg_child:	db "tlsclone: child FS OK", 10
msg_child_len	equ $ - msg_child
msg_c3:		db "tlsclone: clone3 child OK", 10
msg_c3_len	equ $ - msg_c3
msg_fail:	db "tlsclone FAIL", 10
msg_fail_len	equ $ - msg_fail
msg_bad:	db "tlsclone FAIL child TLS", 10
msg_bad_len	equ $ - msg_bad
msg_c3bad:	db "tlsclone FAIL clone3 child", 10
msg_c3bad_len	equ $ - msg_c3bad

section .bss
align 16
tls_parent:	resb 64
tls_child:	resb 64
tls_c3:		resb 64
c3args:		resq 8
align 16
stack1:		resb 4096
stack1_top:
stack2:		resb 4096
stack2_top:
