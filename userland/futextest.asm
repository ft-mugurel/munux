; Phase 6: futex wait/wake + clear_child_tid smoke.
;
; 1) Parent clones a thread with CLONE_CHILD_CLEARTID on `join_slot`.
; 2) Parent FUTEX_WAIT on join_slot while *join_slot == child_tid.
; 3) Child prints, exits → kernel clears join_slot and wakes.
; 4) Parent resumes, checks join_slot==0, prints ok.
;
bits 64
section .text
global _start

%define SYS_WRITE  1
%define SYS_CLONE  56
%define SYS_EXIT   60
%define SYS_WAIT4  61
%define SYS_FUTEX  202

%define FUTEX_WAIT 0
%define FUTEX_WAKE 1
%define FUTEX_PRIVATE 128

; CLONE_VM|CLONE_FILES|CLONE_THREAD|CLONE_CHILD_CLEARTID
%define CLONE_FLAGS 0x00210500

_start:
	; clone(..., child_tid = &join_slot)
	mov rax, SYS_CLONE
	mov rdi, CLONE_FLAGS
	lea rsi, [rel child_stack_top]
	xor rdx, rdx			; parent_tid
	lea r10, [rel join_slot]	; child_tid / clear_child_tid
	xor r8, r8			; tls
	syscall
	test rax, rax
	js .fail
	jz .child

	; ---- parent: join via futex on clear_child_tid word ----
	mov r12, rax			; child tid
	; store expected tid into join_slot if clone did not (CHILD_SETTID not set)
	; kernel may not write settid; child exit clears whatever is there.
	; Musl: while (*ctid == tid) futex_wait(ctid, tid)
	mov dword [rel join_slot], r12d

.wait_join:
	mov eax, dword [rel join_slot]
	cmp eax, r12d
	jne .joined
	mov rax, SYS_FUTEX
	lea rdi, [rel join_slot]
	mov rsi, FUTEX_WAIT | FUTEX_PRIVATE
	mov edx, r12d			; expected == child tid
	xor r10, r10			; no timeout
	xor r8, r8
	syscall
	jmp .wait_join

.joined:
	cmp dword [rel join_slot], 0
	jne .fail_join

	; wait4 to reap zombie (best-effort)
	mov rax, SYS_WAIT4
	mov rdi, r12
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall

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
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_child]
	mov rdx, msg_child_len
	syscall

	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp .hang

.fail_join:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_join]
	mov rdx, msg_join_len
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
join_slot:	dd 0
msg_parent:	db "futextest: parent ok", 10
msg_parent_len	equ $ - msg_parent
msg_child:	db "futextest: child ok", 10
msg_child_len	equ $ - msg_child
msg_fail:	db "futextest: FAIL clone", 10
msg_fail_len	equ $ - msg_fail
msg_join:	db "futextest: FAIL join", 10
msg_join_len	equ $ - msg_join

section .bss
align 16
child_stack:	resb 4096
child_stack_top:
