; CPU exception stubs (vectors 0–31)
; Builds a common stack frame and calls Rust `exception_handler`.
;
; CPU already pushed (same privilege):
;   [ESP] EIP, CS, EFLAGS
; Some vectors also push an error code before that.
;
; We normalize to:
;   pusha regs, vector, error_code, EIP, CS, EFLAGS

bits 32

section .text

extern exception_handler

; ---------------------------------------------------------------------------
; Common path
; ---------------------------------------------------------------------------
isr_common:
	pusha				; EDI ESI EBP ESP EBX EDX ECX EAX

	; Pass pointer to ExceptionFrame (ESP after pusha → &EDI) as cdecl arg
	mov eax, esp
	push eax
	call exception_handler
	; exception_handler never returns. Fallback if it ever did:
	add esp, 4
	popa
	add esp, 8			; pop vector + error_code
	iret

; ---------------------------------------------------------------------------
; Stub macros
; ---------------------------------------------------------------------------
; No CPU error code: push dummy 0, then vector.
%macro ISR_NO_ERR 1
global isr_exception_%1
isr_exception_%1:
	push dword 0
	push dword %1
	jmp isr_common
%endmacro

; CPU already pushed error code: only push vector.
%macro ISR_ERR 1
global isr_exception_%1
isr_exception_%1:
	push dword %1
	jmp isr_common
%endmacro

; Vectors 0–31 (error-code vectors: 8, 10–14, 17)
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
