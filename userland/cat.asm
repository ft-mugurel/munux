; cat [path] — open/read/write/close/exit
; If no path arg, defaults to hello.txt
; Linux x86_64: open=2 read=0 write=1 close=3 exit=60
bits 64
section .text
global _start
_start:
	; Stack from kernel: argc, argv[0], argv[1]?, NULL, env NULL
	; rsp -> argc
	mov rax, [rsp]			; argc
	cmp rax, 2
	jb .use_default
	; argv[1] pointer
	mov rdi, [rsp+16]
	test rdi, rdi
	jz .use_default
	jmp .have_path
.use_default:
	lea rdi, [rel path_default]
.have_path:
	; open(path, O_RDONLY)
	mov rax, 2
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail_open
	mov r12, rax			; fd

.read:
	mov rax, 0
	mov rdi, r12
	lea rsi, [rel buf]
	mov rdx, 256
	syscall
	cmp rax, -4095
	jae .fail_close
	test rax, rax
	jz .done
	mov r13, rax

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
	mov rax, 60
	xor rdi, rdi
	syscall

.fail_close:
	mov rax, 3
	mov rdi, r12
	syscall
.fail_open:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_err]
	mov rdx, msg_err_len
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

section .rodata
path_default:	db "hello.txt", 0
msg_err:	db "cat: cannot open file", 10
msg_err_len equ $ - msg_err

section .bss
align 16
buf:	resb 256
