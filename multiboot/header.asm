; Multiboot 1 header + kernel entry
bits 32

section .multiboot
align 4
	dd 0x1BADB002
	dd 0x00000003
	dd -(0x1BADB002 + 0x00000003)

section .bss
align 4
global multiboot_magic_value
global multiboot_info_addr
multiboot_magic_value: resd 1
multiboot_info_addr:   resd 1

section .text
global _start
extern kmain
extern stack_top

_start:
	cli
	mov esp, stack_top
	mov ebp, esp

	; Save Multiboot registers ASAP (before anything clobbers them).
	; EAX = magic 0x2BADB002, EBX = info struct physical address.
	mov [multiboot_magic_value], eax
	mov [multiboot_info_addr], ebx

	; Early VGA proof-of-life: 'K'
	mov word [0xB8000], 0x0F4B

	call kmain

.hang:
	cli
	hlt
	jmp .hang

global isr_keyboard
extern keyboard_interrupt_handler

isr_keyboard:
	pusha
	call keyboard_interrupt_handler
	popa
	iret
