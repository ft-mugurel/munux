; echotest — smoke /dev/echo from a loaded echo.mnx
; 1) open /dev/echo RDWR, write, read back
; 2) delete_module("echo") while fd open → EBUSY (-16)
; 3) close
; Linux x86_64: open=2 read=0 write=1 close=3 exit=60 delete_module=176
bits 64
section .text
global _start

_start:
	; open("/dev/echo", O_RDWR=2)
	mov rax, 2
	lea rdi, [rel path]
	mov rsi, 2
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .fail_open
	mov r12, rax			; fd

	; write payload
	mov rax, 1
	mov rdi, r12
	lea rsi, [rel payload]
	mov rdx, payload_len
	syscall
	cmp rax, payload_len
	jne .fail_io

	; read back
	mov rax, 0
	mov rdi, r12
	lea rsi, [rel rbuf]
	mov rdx, 64
	syscall
	cmp rax, payload_len
	jne .fail_io

	; compare
	mov rcx, payload_len
	lea rsi, [rel payload]
	lea rdi, [rel rbuf]
	repe cmpsb
	jne .fail_io

	; still open → rmmod must fail EBUSY
	mov rax, 176
	lea rdi, [rel modname]
	xor rsi, rsi
	syscall
	cmp rax, -16			; -EBUSY
	jne .fail_busy

	; close then print PASS
	mov rax, 3
	mov rdi, r12
	syscall

	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_pass]
	mov rdx, msg_pass_len
	syscall
	mov rax, 60
	xor rdi, rdi
	syscall

.fail_open:
	lea rsi, [rel msg_open]
	mov rdx, msg_open_len
	jmp .err
.fail_io:
	mov rax, 3
	mov rdi, r12
	syscall
	lea rsi, [rel msg_io]
	mov rdx, msg_io_len
	jmp .err
.fail_busy:
	mov rax, 3
	mov rdi, r12
	syscall
	lea rsi, [rel msg_busy]
	mov rdx, msg_busy_len
.err:
	mov rax, 1
	mov rdi, 2
	syscall
	mov rax, 60
	mov rdi, 1
	syscall

section .rodata
path:		db "/dev/echo", 0
modname:	db "echo", 0
payload:	db "hello-echo", 10
payload_len	equ $ - payload
msg_pass:	db "echotest: PASS", 10
msg_pass_len	equ $ - msg_pass
msg_open:	db "echotest: open /dev/echo failed", 10
msg_open_len	equ $ - msg_open
msg_io:		db "echotest: read/write mismatch", 10
msg_io_len	equ $ - msg_io
msg_busy:	db "echotest: rmmod not EBUSY while open", 10
msg_busy_len	equ $ - msg_busy

section .bss
align 16
rbuf:	resb 64
