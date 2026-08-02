; Specific test for IRQ preemption under munux.
;
; Parent forks a child; both busy-loop so the timer can switch between them
; while they are Ready/Running (parent will wait at the end).
; After both finish, parent prints a banner and exits.
;
; Check from kernel shell after:  munux> preempt
; Expect irq_preempt_count > 0 if IRQ switched tasks during the loops.
;
bits 64
section .text
global _start

; Busy-loop iterations (~enough wall time for many 100Hz ticks under QEMU)
%define SPIN 80000000

_start:
	; fork
	mov rax, 57
	syscall
	test rax, rax
	js .fail
	jz .child

	; ---- parent: spin, then wait for child ----
	mov rcx, SPIN
.ploop:
	dec rcx
	jnz .ploop

	; wait4(-1, &status, 0, 0)
	mov rax, 61
	mov rdi, -1
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall

	; write "preempttest: parent ok\n"
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_parent]
	mov rdx, msg_parent_len
	syscall

	mov rax, 60
	xor rdi, rdi
	syscall
	jmp .hang

.child:
	mov rcx, SPIN
.cloop:
	dec rcx
	jnz .cloop

	; write "preempttest: child ok\n"
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_child]
	mov rdx, msg_child_len
	syscall

	mov rax, 60
	xor rdi, rdi
	syscall
	jmp .hang

.fail:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

.hang:
	jmp .hang

section .data
status:		dd 0
msg_parent:	db "preempttest: parent ok", 10
msg_parent_len	equ $ - msg_parent
msg_child:	db "preempttest: child ok", 10
msg_child_len	equ $ - msg_child
msg_fail:	db "preempttest: FAIL fork", 10
msg_fail_len	equ $ - msg_fail
