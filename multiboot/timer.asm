bits 32
section .text

global isr_timer
extern timer_interrupt_handler

isr_timer:
	pusha
	call timer_interrupt_handler
	popa
	iret
