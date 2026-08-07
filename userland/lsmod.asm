; lsmod — list loaded modules by reading /proc/modules
; Linux x86_64: open=2 read=0 write=1 close=3 exit=60
bits 64
section .text
global _start

_start:
	; open("/proc/modules", O_RDONLY)
	mov rax, 2
	lea rdi, [rel path]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail
	mov r12, rax

.read:
	mov rax, 0
	mov rdi, r12
	lea rsi, [rel buf]
	mov rdx, 512
	syscall
	cmp rax, -4095
	jae .fail_close
	test rax, rax
	jz .done
	mov r13, rax
	; write(1, buf, n)
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel buf]
	mov rdx, r13
	syscall
	jmp .read

.done:
	mov rax, 3
	mov rdi, r12
	syscall
	; if nothing was printed, show a hint (empty /proc/modules is valid)
	mov rax, 60
	xor rdi, rdi
	syscall

.fail_close:
	mov rax, 3
	mov rdi, r12
	syscall
.fail:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_err]
	mov rdx, msg_err_len
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

section .rodata
path:		db "/proc/modules", 0
msg_err:	db "lsmod: cannot read /proc/modules", 10
msg_err_len equ $ - msg_err

section .bss
align 16
buf:	resb 512
