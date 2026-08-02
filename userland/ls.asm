; ls [path] — open + getdents64 + write
; open=2 getdents64=217 write=1 close=3 exit=60
; Default path is "." (cwd). Supports /proc /dev /ram mount points.
bits 64
section .text
global _start
_start:
	; argc / argv from kernel stack
	mov rax, [rsp]			; argc
	cmp rax, 2
	jb .use_dot
	mov rdi, [rsp+16]		; argv[1]
	test rdi, rdi
	jz .use_dot
	jmp .have_path
.use_dot:
	lea rdi, [rel dot]
.have_path:
	; open(path, O_RDONLY | O_DIRECTORY)
	mov rax, 2
	mov rsi, 0o200000		; O_DIRECTORY
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .try_plain
	mov r12, rax
	jmp .loop

.try_plain:
	; some dirs accept plain O_RDONLY
	mov rax, 2
	; rdi still path
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail
	mov r12, rax

.loop:
	mov rax, 217
	mov rdi, r12
	lea rsi, [rel buf]
	mov rdx, 512
	syscall
	cmp rax, -4095
	jae .fail_close
	test rax, rax
	jz .done
	mov r13, rax			; bytes
	xor r14, r14			; pos

.next_ent:
	cmp r14, r13
	jae .loop
	lea rbx, [rel buf]
	add rbx, r14
	movzx r15, word [rbx + 16]	; d_reclen
	test r15, r15
	jz .done
	lea rsi, [rbx + 19]		; d_name
	xor rdx, rdx
.strlen:
	cmp byte [rsi + rdx], 0
	je .print
	inc rdx
	jmp .strlen
.print:
	; skip . and ..
	cmp rdx, 1
	jne .check2
	cmp byte [rsi], '.'
	je .skip
.check2:
	cmp rdx, 2
	jne .do_print
	cmp word [rsi], 0x2e2e	; ".."
	je .skip
.do_print:
	mov rax, 1
	mov rdi, 1
	syscall
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel nl]
	mov rdx, 1
	syscall
.skip:
	add r14, r15
	jmp .next_ent

.done:
	mov rax, 3
	mov rdi, r12
	syscall
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
dot:	db ".", 0
nl:	db 10
msg_err: db "ls: cannot open", 10
msg_err_len equ $ - msg_err

section .bss
align 16
buf:	resb 512
