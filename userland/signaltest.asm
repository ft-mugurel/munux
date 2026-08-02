; Phase 5: signals
; A) kill(SIGTERM) ends a child (default action)
; B) rt_sigaction handler catches SIGTERM, prints, exits 0
;
bits 64
section .text
global _start

%define SYS_WRITE        1
%define SYS_RT_SIGACTION 13
%define SYS_NANOSLEEP    35
%define SYS_FORK         57
%define SYS_EXIT         60
%define SYS_WAIT4        61
%define SYS_KILL         62
%define SIGTERM          15

_start:
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_start]
	mov rdx, msg_start_len
	syscall

	; ========== Test A: default terminate ==========
	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail
	jz child_spin

	mov r12, rax
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_forked]
	mov rdx, msg_forked_len
	syscall

	; Short sleep so Ready child can be scheduled if needed; then kill.
	mov qword [rel nap_sec], 0
	mov qword [rel nap_nsec], 30000000
	mov rax, SYS_NANOSLEEP
	lea rdi, [rel nap_sec]
	xor rsi, rsi
	syscall

	mov rax, SYS_KILL
	mov rdi, r12
	mov rsi, SIGTERM
	syscall
	test rax, rax
	js fail

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_killed]
	mov rdx, msg_killed_len
	syscall

	mov rax, SYS_WAIT4
	mov rdi, r12
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	test rax, rax
	jle fail
	; Default SIGTERM → exit_code 128+15; wait status is code<<8 (non-zero OK).

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_reaped]
	mov rdx, msg_reaped_len
	syscall

	; ========== Test B: user handler (inherited across fork) ==========
	lea rax, [rel handler]
	mov qword [rel act], rax
	mov qword [rel act+8], 0
	mov qword [rel act+16], 0

	mov rax, SYS_RT_SIGACTION
	mov rdi, SIGTERM
	lea rsi, [rel act]
	xor rdx, rdx
	mov r10, 8
	syscall
	test rax, rax
	js fail

	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail
	jz child_wait_signal

	mov r12, rax
	mov rcx, 2000000
.delay_b:
	dec rcx
	jnz .delay_b

	mov rax, SYS_KILL
	mov rdi, r12
	mov rsi, SIGTERM
	syscall

	mov rax, SYS_WAIT4
	mov rdi, r12
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	test rax, rax
	jle fail
	mov eax, dword [rel status]
	test eax, eax
	jnz fail

	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_ok]
	mov rdx, msg_ok_len
	syscall

	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp hang

; Busy-wait without syscalls never sees pending signals (delivery on syscall
; return / tick for *other* tasks). Sleep so kill/handlers can run.
child_spin:
	mov qword [rel nap_sec], 0
	mov qword [rel nap_nsec], 20000000
	mov rax, SYS_NANOSLEEP
	lea rdi, [rel nap_sec]
	xor rsi, rsi
	syscall
	jmp child_spin

child_wait_signal:
	mov qword [rel nap_sec], 0
	mov qword [rel nap_nsec], 20000000
	mov rax, SYS_NANOSLEEP
	lea rdi, [rel nap_sec]
	xor rsi, rsi
	syscall
	jmp child_wait_signal

; void handler(int sig) — ret → restorer → rt_sigreturn (if not exit)
handler:
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_caught]
	mov rdx, msg_caught_len
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
	jmp hang

fail:
	mov rax, SYS_WRITE
	mov rdi, 2
	lea rsi, [rel msg_fail]
	mov rdx, msg_fail_len
	syscall
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

hang:
	jmp hang

section .data
status:		dd 0
act:		times 32 dq 0
	align 8
nap_sec:	dq 0
nap_nsec:	dq 0
msg_start:	db "signaltest: start", 10
msg_start_len	equ $ - msg_start
msg_forked:	db "signaltest: forked", 10
msg_forked_len	equ $ - msg_forked
msg_killed:	db "signaltest: killed", 10
msg_killed_len	equ $ - msg_killed
msg_reaped:	db "signaltest: reaped", 10
msg_reaped_len	equ $ - msg_reaped
msg_ok:		db "signaltest: parent ok", 10
msg_ok_len	equ $ - msg_ok
msg_caught:	db "signaltest: caught", 10
msg_caught_len	equ $ - msg_caught
msg_fail:	db "signaltest: FAIL", 10
msg_fail_len	equ $ - msg_fail
