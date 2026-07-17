; munux x86_64 CPU exception stubs (vectors 0-31)
; Builds a common frame and calls Rust exception_handler.

bits 64
default abs

section .text

extern exception_handler

; ---------------------------------------------------------------------------
; Common path
; After stub: [RSP] = vector, [RSP+8] = error_code, then CPU frame
; We push GPRs so ExceptionFrame starts at RAX (lowest address).
; ---------------------------------------------------------------------------
isr_common:
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

	; System V AMD64: rdi = first arg = pointer to frame
	mov rdi, rsp
	; 16-byte align stack before call
	mov rbp, rsp
	and rsp, -16
	call exception_handler
	; never returns; if it did:
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
	add rsp, 16			; pop vector + error
	iretq

; ---------------------------------------------------------------------------
; Stub macros
; ---------------------------------------------------------------------------
%macro ISR_NO_ERR 1
global isr_exception_%1
isr_exception_%1:
	push qword 0			; dummy error code
	push qword %1			; vector
	jmp isr_common
%endmacro

%macro ISR_ERR 1
global isr_exception_%1
isr_exception_%1:
	push qword %1			; vector (error already on stack)
	jmp isr_common
%endmacro

; Vectors 0-31 (error-code: 8, 10-14, 17)
ISR_NO_ERR 0
ISR_NO_ERR 1
ISR_NO_ERR 2
ISR_NO_ERR 3
ISR_NO_ERR 4
ISR_NO_ERR 5
ISR_NO_ERR 6
ISR_NO_ERR 7
ISR_ERR    8
ISR_NO_ERR 9
ISR_ERR    10
ISR_ERR    11
ISR_ERR    12
ISR_ERR    13
ISR_ERR    14
ISR_NO_ERR 15
ISR_NO_ERR 16
ISR_ERR    17
ISR_NO_ERR 18
ISR_NO_ERR 19
ISR_NO_ERR 20
ISR_NO_ERR 21
ISR_NO_ERR 22
ISR_NO_ERR 23
ISR_NO_ERR 24
ISR_NO_ERR 25
ISR_NO_ERR 26
ISR_NO_ERR 27
ISR_NO_ERR 28
ISR_NO_ERR 29
ISR_NO_ERR 30
ISR_NO_ERR 31
