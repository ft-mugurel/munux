; munux x86_64 hardware IRQ stubs (PIC-remapped vectors)
; IRQ0 timer  -> vector 32
; IRQ1 keyboard -> vector 33

bits 64
default abs

section .text

extern timer_interrupt_handler
extern keyboard_interrupt_handler

; ---------------------------------------------------------------------------
; IRQ0 — PIT timer
; ---------------------------------------------------------------------------
global isr_timer
isr_timer:
	push r15
	push r14
	push r13
	push r12
	push r11
	push r10
	push r9
	push r8
	push rbp
	push rdi
	push rsi
	push rdx
	push rcx
	push rbx
	push rax

	mov rbp, rsp
	and rsp, -16
	call timer_interrupt_handler
	mov rsp, rbp

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
; IRQ1 — PS/2 keyboard
; ---------------------------------------------------------------------------
global isr_keyboard
isr_keyboard:
	push r15
	push r14
	push r13
	push r12
	push r11
	push r10
	push r9
	push r8
	push rbp
	push rdi
	push rsi
	push rdx
	push rcx
	push rbx
	push rax

	mov rbp, rsp
	and rsp, -16
	call keyboard_interrupt_handler
	mov rsp, rbp

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
