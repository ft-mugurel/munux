; munux - Multiboot2 header + 32-bit trampoline -> long mode -> kmain
; Assembled as ELF64; Multiboot enters in 32-bit protected mode.
;
; Addresses use KLOAD+(label-$$) so NASM emits immediates (no abs relocs).
; Matches linker script: . = 1M; *(.text)

bits 32

%define KLOAD 0x100000

section .text

; ---------------------------------------------------------------------------
; Multiboot2 header (first bytes of .text / file image after ELF headers)
; ---------------------------------------------------------------------------
align 8
mb2_header_start:
	dd 0xE85250D6
	dd 0
	dd mb2_header_end - mb2_header_start
	dd 0x100000000 - (0xE85250D6 + 0 + (mb2_header_end - mb2_header_start))
	dw 0
	dw 0
	dd 8
mb2_header_end:

; ---------------------------------------------------------------------------
; Early page tables + stacks + GDT
; ---------------------------------------------------------------------------
align 4096
boot_pml4:
	times 4096 db 0
boot_pdpt:
	times 4096 db 0
boot_pd:
	times 4096 db 0

align 16
global multiboot_magic_value
global multiboot_info_addr
multiboot_magic_value:
	dd 0
multiboot_info_addr:
	dd 0

align 16
boot_stack_bottom:
	times 4096 db 0
boot_stack_top:

align 16
kstack_bottom:
	times 65536 db 0
kstack_top:

align 16
gdt64:
	dq 0
	dq 0x00AF9A000000FFFF
	dq 0x00AF92000000FFFF
gdt64_end:

align 8
gdt64_ptr:
	dw gdt64_end - gdt64 - 1
	dd KLOAD + (gdt64 - $$)

; ---------------------------------------------------------------------------
; Entry
; ---------------------------------------------------------------------------
global _start
extern kmain

_start:
	cli
	mov dword [KLOAD + (multiboot_magic_value - $$)], eax
	mov dword [KLOAD + (multiboot_info_addr - $$)], ebx

	mov esp, KLOAD + (boot_stack_top - $$)
	mov ebp, esp

	mov word [0xB8000], 0x0F4D

	mov edi, KLOAD + (boot_pml4 - $$)
	xor eax, eax
	mov ecx, (4096 * 3) / 4
	rep stosd

	mov eax, KLOAD + (boot_pdpt - $$)
	or eax, 0x03
	mov dword [KLOAD + (boot_pml4 - $$)], eax

	mov eax, KLOAD + (boot_pd - $$)
	or eax, 0x03
	mov dword [KLOAD + (boot_pdpt - $$)], eax

	mov edi, KLOAD + (boot_pd - $$)
	xor ecx, ecx
.map_pd:
	mov eax, ecx
	shl eax, 21
	or eax, 0x83
	mov [edi + ecx * 8], eax
	inc ecx
	cmp ecx, 512
	jb .map_pd

	mov eax, cr4
	or eax, 1 << 5
	mov cr4, eax

	mov eax, KLOAD + (boot_pml4 - $$)
	mov cr3, eax

	mov ecx, 0xC0000080
	rdmsr
	or eax, 1 << 8
	wrmsr

	mov eax, cr0
	or eax, 1 << 31
	or eax, 1
	mov cr0, eax

	lgdt [KLOAD + (gdt64_ptr - $$)]
	jmp 0x08:KLOAD + (long_mode_start - $$)

bits 64
default abs

long_mode_start:
	mov ax, 0x10
	mov ds, ax
	mov es, ax
	mov ss, ax
	mov fs, ax
	mov gs, ax

	mov rsp, KLOAD + (kstack_top - $$)
	xor rbp, rbp

	mov word [0xB8000 + 2], 0x0A75

	and rsp, -16
	; PC-relative call into Rust (same final .text after link)
	call kmain

.hang:
	cli
	hlt
	jmp .hang
