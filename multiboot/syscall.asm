; int 0x80 syscall entry / leave user mode helpers
bits 32
section .text

global isr_syscall
global enter_user_mode
global return_from_user

extern syscall_dispatch
extern tss_set_esp0_from_esp

; Saved kernel ESP so SYS_EXIT can return into enter_user_mode's caller.
; Points at the 4 callee-saved regs we pushed (edi,esi,ebx,ebp) then retaddr.
saved_kernel_esp:
	dd 0

; ---------------------------------------------------------------------------
; Syscall gate (vector 0x80)
; User: EAX=num EBX=a1 ECX=a2 EDX=a3 ESI=a4 EDI=a5
; Returns EAX=result
; ---------------------------------------------------------------------------
isr_syscall:
	; Save user segments / general regs
	push ds
	push es
	push fs
	push gs
	pusha				; EDI ESI EBP ESP EBX EDX ECX EAX (user order)

	; Kernel data segments
	mov ax, 0x10
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax

	; Args for cdecl syscall_dispatch(num, a1, a2, a3, a4, a5)
	; After pusha, EAX is at [esp+28], EBX at [esp+16], ECX [esp+24], EDX [esp+20], ESI [esp+4], EDI [esp]
	mov eax, [esp + 28]		; syscall number (saved EAX)
	mov ebx, [esp + 16]		; arg1
	mov ecx, [esp + 24]		; arg2
	mov edx, [esp + 20]		; arg3
	mov esi, [esp + 4]		; arg4
	mov edi, [esp]			; arg5

	push edi
	push esi
	push edx
	push ecx
	push ebx
	push eax
	call syscall_dispatch
	add esp, 24
	; EAX = return value — store into saved EAX on pusha frame
	mov [esp + 28], eax

	popa
	pop gs
	pop fs
	pop es
	pop ds
	iret

; ---------------------------------------------------------------------------
; enter_user_mode(entry: u32, user_esp: u32)  cdecl
; Does not return until user calls exit → return_from_user
;
; MUST preserve callee-saved regs (EBX/ESI/EDI/EBP): SYS_EXIT jumps here via
; return_from_user without going through a normal epilogue, and user/syscall
; path clobbers those registers. Leaving EBP=0 caused a page fault in the
; Rust caller (frame-pointer addressing → CR2=0xffffffa2).
; ---------------------------------------------------------------------------
enter_user_mode:
	; [esp] ret, [esp+4] entry, [esp+8] user_esp
	cli				; no IRQs while we build the ring-3 frame
	push ebp
	push ebx
	push esi
	push edi
	; stack: edi, esi, ebx, ebp, ret, entry, user_esp
	mov [saved_kernel_esp], esp

	; TSS.esp0 = current ESP so ring-3 IRQs push below our saved frame
	mov eax, esp
	push eax
	call tss_set_esp0_from_esp
	add esp, 4

	; Offsets after the 4 pushes:
	; [esp+16]=ret [esp+20]=entry [esp+24]=user_esp
	mov eax, [esp + 20]		; entry EIP
	mov ebx, [esp + 24]		; user ESP

	; Build iret frame: SS, ESP, EFLAGS, CS, EIP
	push dword 0x33			; user SS (RPL=3)
	push ebx			; user ESP
	pushf
	or dword [esp], 0x200		; IF=1 in user mode
	push dword 0x23			; user CS (RPL=3)
	push eax			; user EIP

	; User data segments
	mov ax, 0x2B
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax

	iret

; ---------------------------------------------------------------------------
; return_from_user — jump back to kernel after SYS_EXIT (does not return here)
; Restores ESP + callee-saved regs saved in enter_user_mode, then RET.
; ---------------------------------------------------------------------------
return_from_user:
	cli
	mov ax, 0x10
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax
	mov ax, 0x18
	mov ss, ax
	mov esp, [saved_kernel_esp]
	pop edi
	pop esi
	pop ebx
	pop ebp
	; STI is done in Rust after enter_user_mode returns (int gate clears IF)
	ret
