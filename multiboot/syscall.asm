; munux x86_64: syscall entry + enter_user_mode / return_from_user
;
; SYSCALL: RCX=user RIP, R11=user RFLAGS, RSP still user stack until we switch
; SYSRETQ: RIP=RCX, RFLAGS=R11, CS/SS from STAR; RSP must be set by us
;
; U6: nested enter_user_mode (fork/wait/execve), last-user context for fork

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
; Nested enter_user_mode return frames (shell → wait → exec, …)
MAX_ENTER_NEST equ 8
saved_kernel_rsp_depth:
	dq 0
saved_kernel_rsp_stack:
	times MAX_ENTER_NEST dq 0
; Snapshot at syscall entry (for fork)
last_user_rip:
	dq 0
last_user_rsp:
	dq 0
last_user_rflags:
	dq 0
; 6th syscall arg (user r9) — avoid SysV stack arg for a6 in Rust.
last_user_r9:
	dq 0

section .text

global syscall_entry
global enter_user_mode
global return_from_user
global set_syscall_kstack
global last_user_rip
global last_user_rsp
global last_user_rflags
global last_user_r9
global get_enter_nest_depth
global resume_user_trap

extern syscall_dispatch

; Called from Rust: set_syscall_kstack(u64)
set_syscall_kstack:
	mov [rel syscall_kstack], rdi
	ret

; u64 get_enter_nest_depth(void) — how many enter_user_mode frames are active
get_enter_nest_depth:
	mov rax, [rel saved_kernel_rsp_depth]
	ret

; resume_user_trap(rdi = *TrapFrame) — never returns; iretq into user
; TrapFrame layout matches IRQ push order (rax first … ss last).
resume_user_trap:
	cli
	; Kernel → user data segments
	mov ax, 0x1B
	mov ds, ax
	mov es, ax
	; Copy frame to stack so pops are safe if rdi points into a PCB
	sub rsp, 160			; 20 * 8
	mov rsi, rdi
	mov rdi, rsp
	mov rcx, 20
	rep movsq
	; Now stack top = TrapFrame
	pop rax
	pop rbx
	pop rcx
	pop rdx
	pop rsi
	pop rdi
	pop rbp
	pop r8
	pop r9
	pop r10
	pop r11
	pop r12
	pop r13
	pop r14
	pop r15
	iretq

; ---------------------------------------------------------------------------
; syscall_entry — LSTAR
; ---------------------------------------------------------------------------
syscall_entry:
	; Snapshot user context for fork before switching stacks
	mov [rel last_user_rsp], rsp
	mov [rel last_user_rip], rcx
	mov [rel last_user_rflags], r11
	mov [rel last_user_r9], r9	; 6th arg (futex val3, …)

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

	; Musl/C keep heap pointers in callee-saved regs across syscall (esp. rbp).
	; Rust syscall_dispatch clobbers them — must save/restore.
	push rbx
	push rbp
	push r12
	push r13
	push r14
	push r15

	; Stack layout (rsp → high):
	; [0..40] r15..rbx, [48]=num, [56]=rdi, [64]=rsi, [72]=rdx,
	; [80]=r10, [88]=r8, [96]=r9, [104]=rip, [112]=rflags
	; Pass 6 C args in registers only (SysV). User r9 is in last_user_r9.
	mov rdi, [rsp + 48]		; num
	mov rsi, [rsp + 56]		; rdi user
	mov rdx, [rsp + 64]		; rsi user
	mov rcx, [rsp + 72]		; rdx user
	mov r8,  [rsp + 80]		; r10 user
	mov r9,  [rsp + 88]		; r8 user

	mov rbp, rsp
	and rsp, -16
	call syscall_dispatch
	mov rsp, rbp
	; rax = return value

	pop r15
	pop r14
	pop r13
	pop r12
	pop rbp
	pop rbx

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

; Optional first user arg (e.g. signal number for handler entry)
global enter_user_rdi
section .bss
align 8
enter_user_rdi:	resq 1
section .text

; ---------------------------------------------------------------------------
; enter_user_mode(entry, user_rsp, user_rax)
; SysV: rdi=entry, rsi=user_rsp, rdx=initial user rax
; Nested: pushes kernel frame; return_from_user pops one level
; Loads user RDI from enter_user_rdi (signal handlers need rdi=sig).
; ---------------------------------------------------------------------------
enter_user_mode:
	; Preserve callee-saved for return to kernel caller
	push rbp
	push rbx
	push r12
	push r13
	push r14
	push r15
	mov r8, rdx			; save initial user rax

	; Nest: store this frame's RSP
	mov rax, [rel saved_kernel_rsp_depth]
	cmp rax, MAX_ENTER_NEST
	jae .nest_full
	lea rbx, [rel saved_kernel_rsp_stack]
	mov [rbx + rax*8], rsp
	inc qword [rel saved_kernel_rsp_depth]

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

	; Initial user regs
	mov rax, r8
	xor rbx, rbx
	xor rcx, rcx
	xor rdx, rdx
	xor rsi, rsi
	mov rdi, [rel enter_user_rdi]	; signal number or 0
	xor rbp, rbp
	xor r8, r8
	xor r9, r9
	xor r10, r10
	xor r11, r11
	xor r12, r12
	xor r13, r13
	xor r14, r14
	xor r15, r15

	; clear sticky enter_user_rdi after consume
	mov qword [rel enter_user_rdi], 0

	iretq

.nest_full:
	; Should not happen; restore and return
	pop r15
	pop r14
	pop r13
	pop r12
	pop rbx
	pop rbp
	ret

; ---------------------------------------------------------------------------
; return_from_user — SYS_EXIT (and failed/finished execve) jumps here
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

	mov rax, [rel saved_kernel_rsp_depth]
	test rax, rax
	jz .no_frame
	dec rax
	mov [rel saved_kernel_rsp_depth], rax
	lea rbx, [rel saved_kernel_rsp_stack]
	mov rsp, [rbx + rax*8]
	pop r15
	pop r14
	pop r13
	pop r12
	pop rbx
	pop rbp
	; sti done in Rust after enter_user_mode returns
	ret

.no_frame:
.hang:
	hlt
	jmp .hang
