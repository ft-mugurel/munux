; Phase 4: clone (CLONE_VM | CLONE_THREAD) + gettid smoke test.
;
; Parent clones a thread with a private stack buffer (shared AS).
; Child sets a shared flag, prints, exits.
; Parent waits for the flag, checks getpid == parent gettid (leader),
; gettid of parent != child tid (from clone return), then wait4 + print.
;
bits 64
section .text
global _start

; Linux x86_64
%define SYS_WRITE   1
%define SYS_CLONE   56
%define SYS_EXIT    60
%define SYS_WAIT4   61
%define SYS_GETPID  39
%define SYS_GETTID  186

; CLONE_VM | CLONE_FILES | CLONE_THREAD  (pthread-like)
%define CLONE_FLAGS 0x00010500

_start:
	; clone(flags, stack, 0, 0, 0)
	mov rax, SYS_CLONE
	mov rdi, CLONE_FLAGS
	lea rsi, [rel child_stack_top]
	xor rdx, rdx
	xor r10, r10
	xor r8, r8
	syscall
	test rax, rax
	js .fail
	jz .child

	; ---- parent (thread group leader) ----
	mov r12, rax			; child tid

	; getpid must equal gettid for the leader
	mov rax, SYS_GETPID
	syscall
	mov r13, rax
	mov rax, SYS_GETTID
	syscall
	cmp rax, r13
	jne .fail_ids
	; child tid must differ from our tid
	cmp r12, rax
	je .fail_ids

	; wait4 schedules the Ready child (nest depth ≥2: no IRQ preempt).
	; CLONE_THREAD tasks may be auto-reaped on exit (join via futex); wait4
	; can return -ECHILD after the child already ran — still OK if flag set.
	mov rax, SYS_WAIT4
	mov rdi, r12
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall

	; child should have set the shared flag before exit
	cmp dword [rel shared_flag], 1
	jne .fail_ids

	; write "clonetest: parent ok\n"
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_parent]
	mov rdx, msg_parent_len
	syscall

	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp .hang

.child:
	; mark shared flag (CLONE_VM shared AS)
	mov dword [rel shared_flag], 1

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_child]
	mov rdx, msg_child_len
	syscall

	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp .hang

.fail_ids:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_ids]
	mov rdx, msg_ids_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 2
	syscall
	jmp .hang

.fail:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

.hang:
	jmp .hang

section .data
status:		dd 0
shared_flag:	dd 0
msg_parent:	db "clonetest: parent ok", 10
msg_parent_len	equ $ - msg_parent
msg_child:	db "clonetest: child ok", 10
msg_child_len	equ $ - msg_child
msg_fail:	db "clonetest: FAIL clone", 10
msg_fail_len	equ $ - msg_fail
msg_ids:	db "clonetest: FAIL tid/pid", 10
msg_ids_len	equ $ - msg_ids

section .bss
align 16
child_stack:	resb 4096
child_stack_top:
