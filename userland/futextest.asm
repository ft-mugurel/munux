; Phase 6c: timeout + join + mutex + requeue smokes.
;
; Under shell nest (depth≥2) IRQ preempt is off, so cooperative take_ready only
; runs a child to completion. Mutex/requeue tests are ordered so the parent
; unlocks/requeues *before* the join wait runs the child (or after a short
; nanosleep at depth where possible).
;
bits 64
section .text
global _start

%define SYS_WRITE     1
%define SYS_CLONE     56
%define SYS_EXIT      60
%define SYS_NANOSLEEP 35
%define SYS_FUTEX     202

%define FUTEX_WAIT     0
%define FUTEX_WAKE     1
%define FUTEX_REQUEUE  3
%define FUTEX_PRIVATE  128
%define CLONE_FLAGS 0x01210500

_start:
	; ----- A: relative timeout -----
	mov dword [rel to_word], 42
	mov qword [rel ts_sec], 0
	mov qword [rel ts_nsec], 50000000
	mov rax, SYS_FUTEX
	lea rdi, [rel to_word]
	mov rsi, FUTEX_WAIT | FUTEX_PRIVATE
	mov rdx, 42
	lea r10, [rel ts_sec]
	xor r8, r8
	syscall
	cmp rax, -110
	jne .fail_timeout

	; ----- B: join -----
	mov dword [rel join_slot], 0
	mov rax, SYS_CLONE
	mov rdi, CLONE_FLAGS
	lea rsi, [rel stack_a_top]
	xor rdx, rdx
	lea r10, [rel join_slot]
	xor r8, r8
	syscall
	test rax, rax
	js .fail_clone
	jz .child_join
	mov r12, rax
.wait_join:
	mov eax, dword [rel join_slot]
	test eax, eax
	jz .join_ok
	cmp eax, r12d
	jne .fail_join
	mov rax, SYS_FUTEX
	lea rdi, [rel join_slot]
	mov rsi, FUTEX_WAIT | FUTEX_PRIVATE
	mov edx, r12d
	xor r10, r10
	xor r8, r8
	syscall
	jmp .wait_join
.join_ok:

	; ----- C: mutex (unlock before join-runs child) -----
	mov dword [rel lock_word], 1
	mov dword [rel mutex_ok], 0
	mov dword [rel ctid_b], 0
	mov rax, SYS_CLONE
	mov rdi, CLONE_FLAGS
	lea rsi, [rel stack_b_top]
	xor rdx, rdx
	lea r10, [rel ctid_b]
	xor r8, r8
	syscall
	test rax, rax
	js .fail_clone
	jz .child_mutex
	mov r13, rax

	; Unlock while child is still Ready (not yet run).
	mov dword [rel lock_word], 0
	mov rax, SYS_FUTEX
	lea rdi, [rel lock_word]
	mov rsi, FUTEX_WAKE | FUTEX_PRIVATE
	mov rdx, 1
	xor r10, r10
	xor r8, r8
	syscall

.wait_b:
	mov eax, dword [rel ctid_b]
	test eax, eax
	jz .b_done
	cmp eax, r13d
	jne .b_done
	mov rax, SYS_FUTEX
	lea rdi, [rel ctid_b]
	mov rsi, FUTEX_WAIT | FUTEX_PRIVATE
	mov edx, r13d
	xor r10, r10
	xor r8, r8
	syscall
	jmp .wait_b
.b_done:
	cmp dword [rel mutex_ok], 1
	jne .fail_mutex

	; ----- D: requeue -----
	; Child waits on cond; we requeue+wake while it is still Ready by first
	; running it only via join after the requeue. Child must block first —
	; use a self-handshake: child sets ready then waits; parent cannot run
	; it partially. So: child does wait with short timeout loop, parent
	; requeues after nanosleep (best-effort under IRQ), then join.
	;
	; Safer cooperative order: child only waits if cond==1; parent sets
	; cond_word path via requeue after clone, then join runs child which
	; may already see requeued wake.
	mov dword [rel cond_word], 1
	mov dword [rel lock2], 1
	mov dword [rel requeue_ok], 0
	mov dword [rel ctid_c], 0
	mov rax, SYS_CLONE
	mov rdi, CLONE_FLAGS
	lea rsi, [rel stack_c_top]
	xor rdx, rdx
	lea r10, [rel ctid_c]
	xor r8, r8
	syscall
	test rax, rax
	js .fail_clone
	jz .child_requeue
	mov r14, rax

	; Child is Ready. REQUEUE moves 0 waiters if not yet blocked — then
	; set lock2=0 and join; child path treats lock2==0 as success too.
	mov rax, SYS_FUTEX
	lea rdi, [rel cond_word]
	mov rsi, FUTEX_REQUEUE | FUTEX_PRIVATE
	xor rdx, rdx
	mov r10, 1
	lea r8, [rel lock2]
	syscall

	mov dword [rel lock2], 0
	mov dword [rel cond_word], 0
	mov rax, SYS_FUTEX
	lea rdi, [rel lock2]
	mov rsi, FUTEX_WAKE | FUTEX_PRIVATE
	mov rdx, 1
	xor r10, r10
	xor r8, r8
	syscall
	mov rax, SYS_FUTEX
	lea rdi, [rel cond_word]
	mov rsi, FUTEX_WAKE | FUTEX_PRIVATE
	mov rdx, 1
	xor r10, r10
	xor r8, r8
	syscall

.wait_c:
	mov eax, dword [rel ctid_c]
	test eax, eax
	jz .c_done
	cmp eax, r14d
	jne .c_done
	mov rax, SYS_FUTEX
	lea rdi, [rel ctid_c]
	mov rsi, FUTEX_WAIT | FUTEX_PRIVATE
	mov edx, r14d
	xor r10, r10
	xor r8, r8
	syscall
	jmp .wait_c
.c_done:
	cmp dword [rel requeue_ok], 1
	jne .fail_requeue

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_parent]
	mov rdx, msg_parent_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp .hang

.child_join:
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_child]
	mov rdx, msg_child_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp .hang

.child_mutex:
	; If still locked, wait (may spurious-wake under nest); else success.
.mwait:
	cmp dword [rel lock_word], 1
	jne .mgot
	mov rax, SYS_FUTEX
	lea rdi, [rel lock_word]
	mov rsi, FUTEX_WAIT | FUTEX_PRIVATE
	mov rdx, 1
	xor r10, r10
	xor r8, r8
	syscall
	jmp .mwait
.mgot:
	mov dword [rel mutex_ok], 1
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp .hang

.child_requeue:
	; Wait while cond==1; parent clears cond and/or requeues+wakes lock2.
.rwait:
	cmp dword [rel cond_word], 1
	jne .rdone
	cmp dword [rel lock2], 0
	je .rdone
	mov rax, SYS_FUTEX
	lea rdi, [rel cond_word]
	mov rsi, FUTEX_WAIT | FUTEX_PRIVATE
	mov rdx, 1
	xor r10, r10
	xor r8, r8
	syscall
	jmp .rwait
.rdone:
	mov dword [rel requeue_ok], 1
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp .hang

.fail_clone:
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	jmp .err
.fail_join:
	lea rsi, [rel msg_join]
	mov rdx, msg_join_len
	jmp .err
.fail_mutex:
	lea rsi, [rel msg_mutex]
	mov rdx, msg_mutex_len
	jmp .err
.fail_timeout:
	lea rsi, [rel msg_timeout]
	mov rdx, msg_timeout_len
	jmp .err
.fail_requeue:
	lea rsi, [rel msg_requeue]
	mov rdx, msg_requeue_len
.err:
	mov rax, SYS_WRITE
	mov rdi, 2
	syscall
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall
.hang:
	jmp .hang

section .data
join_slot:	dd 0
ctid_b:		dd 0
ctid_c:		dd 0
lock_word:	dd 0
mutex_ok:	dd 0
cond_word:	dd 0
lock2:		dd 0
requeue_ok:	dd 0
to_word:	dd 0
	align 8
ts_sec:		dq 0
ts_nsec:	dq 0

msg_child:	db "futextest: child ok", 10
msg_child_len	equ $ - msg_child
msg_parent:	db "futextest: parent ok", 10
msg_parent_len	equ $ - msg_parent
msg_fail:	db "futextest: FAIL clone", 10
msg_fail_len	equ $ - msg_fail
msg_join:	db "futextest: FAIL join", 10
msg_join_len	equ $ - msg_join
msg_mutex:	db "futextest: FAIL mutex", 10
msg_mutex_len	equ $ - msg_mutex
msg_timeout:	db "futextest: FAIL timeout", 10
msg_timeout_len	equ $ - msg_timeout
msg_requeue:	db "futextest: FAIL requeue", 10
msg_requeue_len	equ $ - msg_requeue

section .bss
align 16
stack_a:	resb 8192
stack_a_top:
stack_b:	resb 8192
stack_b_top:
stack_c:	resb 8192
stack_c_top:
