; munux x86_64: syscall entry + enter_user_mode / return_from_user
;
; SYSCALL: RCX=user RIP, R11=user RFLAGS, RSP still user stack until we switch
; SYSRETQ: RIP=RCX, RFLAGS=R11, CS/SS from STAR; RSP must be set by us

bits 64
default abs

section .data
align 8
; Kernel stack pointer used while handling syscalls from ring 3
syscall_kstack:
	dq 0
; User RSP saved across syscall
saved_user_rsp:
	dq 0
; Kernel RSP for return_from_user (SYS_EXIT → shell)
saved_kernel_rsp:
	dq 0

section .text

global syscall_entry
global enter_user_mode
global return_from_user
global set_syscall_kstack

extern syscall_dispatch

; Called from Rust: set_syscall_kstack(u64)
set_syscall_kstack:
	mov [rel syscall_kstack], rdi
	ret

; ---------------------------------------------------------------------------
; syscall_entry — LSTAR
; ---------------------------------------------------------------------------
syscall_entry:
	; Save user stack pointer
	mov [rel saved_user_rsp], rsp

	; Kernel stack
	mov rsp, [rel syscall_kstack]

	; Build arg frame / preserve user state
	push r11			; user rflags
	push rcx			; user rip
	push r9
	push r8
	push r10
	push rdx
	push rsi
	push rdi
	push rax			; syscall number

	; dispatch(num, a1, a2, a3, a4, a5)  — Linux-like regs
	mov rdi, [rsp]			; num
	mov rsi, [rsp + 8]		; rdi user
	mov rdx, [rsp + 16]		; rsi user
	mov rcx, [rsp + 24]		; rdx user
	mov r8,  [rsp + 32]		; r10 user
	mov r9,  [rsp + 40]		; r8 user

	mov rbp, rsp
	and rsp, -16
	call syscall_dispatch
	mov rsp, rbp
	; rax = return value

	add rsp, 8			; pop num
	pop rdi
	pop rsi
	pop rdx
	pop r10
	pop r8
	pop r9
	pop rcx				; user rip for sysret
	pop r11				; user rflags for sysret

	mov rsp, [rel saved_user_rsp]
	o64 sysret

; ---------------------------------------------------------------------------
; enter_user_mode(entry, user_rsp)  SysV: rdi=entry, rsi=user_rsp
; Does not return until user SYS_EXIT → return_from_user
; ---------------------------------------------------------------------------
enter_user_mode:
	; Preserve callee-saved for shell return
	push rbp
	push rbx
	push r12
	push r13
	push r14
	push r15
	mov [rel saved_kernel_rsp], rsp

	; syscall_kstack is set by Rust to a dedicated kernel stack
	; (must NOT reuse this shell frame — EXIT restores saved_kernel_rsp)

	; iretq frame: SS, RSP, RFLAGS, CS, RIP
	; user SS = 0x1B, user CS = 0x23
	push qword 0x1B			; SS
	push rsi			; user RSP
	pushfq
	or qword [rsp], 0x200		; IF=1 in user
	push qword 0x23			; CS
	push rdi			; entry RIP

	; User data segments
	mov ax, 0x1B
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax

	iretq

; ---------------------------------------------------------------------------
; return_from_user — SYS_EXIT jumps here (does not return to caller of this)
; ---------------------------------------------------------------------------
return_from_user:
	cli
	; Kernel segments
	mov ax, 0x10
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax
	mov ss, ax

	mov rsp, [rel saved_kernel_rsp]
	pop r15
	pop r14
	pop r13
	pop r12
	pop rbx
	pop rbp
	; sti done in Rust after enter_user_mode returns
	ret
