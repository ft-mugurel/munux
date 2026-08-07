; P9c: poll / select / epoll smoke
bits 64
section .text
global _start

%define SYS_WRITE		1
%define SYS_CLOSE		3
%define SYS_POLL		7
%define SYS_PIPE		22
%define SYS_SELECT		23
%define SYS_EXIT		60
%define SYS_WAIT4		61
%define SYS_FORK		57
%define SYS_EPOLL_CTL		233
%define SYS_EPOLL_WAIT		232
%define SYS_EPOLL_CREATE1	291

%define POLLIN		1
%define POLLOUT		4
%define POLLNVAL	0x20
%define EPOLLIN		1
%define EPOLL_CTL_ADD	1

%macro puts 2
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel %1]
	mov rdx, %2
	syscall
%endmacro

_start:
	; ---- A: stdout POLLOUT, timeout 0 ----
	mov dword [rel pfd + 0], 1		; fd = 1
	mov word  [rel pfd + 4], POLLOUT
	mov word  [rel pfd + 6], 0
	mov rax, SYS_POLL
	lea rdi, [rel pfd]
	mov rsi, 1
	xor rdx, rdx
	syscall
	cmp rax, 1
	jne fail_a
	movzx ebx, word [rel pfd + 6]
	test ebx, POLLOUT
	jz fail_a
	puts msg_a, msg_a_len

	; ---- B: empty pipe POLLIN timeout ~40ms → 0 ----
	mov rax, SYS_PIPE
	lea rdi, [rel pipefd]
	syscall
	test rax, rax
	js fail_b
	mov eax, [rel pipefd]			; read end
	mov [rel pfd], eax
	mov word [rel pfd + 4], POLLIN
	mov word [rel pfd + 6], 0
	mov rax, SYS_POLL
	lea rdi, [rel pfd]
	mov rsi, 1
	mov rdx, 40
	syscall
	test rax, rax
	jnz fail_b
	puts msg_b, msg_b_len
	; leave pipefds open for later tests? close them
	mov eax, [rel pipefd]
	mov edi, eax
	mov rax, SYS_CLOSE
	syscall
	mov eax, [rel pipefd + 4]
	mov edi, eax
	mov rax, SYS_CLOSE
	syscall

	; ---- C: fork + pipe, parent poll until child writes ----
	mov rax, SYS_PIPE
	lea rdi, [rel pipefd]
	syscall
	test rax, rax
	js fail_c
	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail_c
	jz child_c
	; parent
	mov eax, [rel pipefd]
	mov [rel pfd], eax
	mov word [rel pfd + 4], POLLIN
	mov word [rel pfd + 6], 0
	mov rax, SYS_POLL
	lea rdi, [rel pfd]
	mov rsi, 1
	mov rdx, 2000
	syscall
	cmp rax, 1
	jne fail_c
	movzx ebx, word [rel pfd + 6]
	test ebx, POLLIN
	jz fail_c
	mov rax, SYS_WAIT4
	mov rdi, -1
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	puts msg_c, msg_c_len
	jmp do_d

child_c:
	mov eax, [rel pipefd + 4]
	mov edi, eax
	mov rax, SYS_WRITE
	lea rsi, [rel byte_x]
	mov rdx, 1
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

do_d:
	; ---- D: select() on a fresh pipe + fork ----
	mov rax, SYS_PIPE
	lea rdi, [rel pipefd]
	syscall
	test rax, rax
	js fail_d
	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail_d
	jz child_d
	; zero fd_set (16 bytes enough)
	xor rax, rax
	mov [rel fdset], rax
	mov [rel fdset + 8], rax
	mov eax, [rel pipefd]			; read fd
	mov ecx, eax
	mov ebx, 1
	shl rbx, cl				; 1 << fd  (fd < 32)
	mov [rel fdset], rbx
	lea rdi, [rel timeval]
	mov qword [rdi], 2			; 2 seconds
	mov qword [rdi + 8], 0
	mov rax, SYS_SELECT
	mov edi, [rel pipefd]
	inc edi					; nfds = readfd+1
	lea rsi, [rel fdset]			; readfds
	xor rdx, rdx
	xor r10, r10
	lea r8, [rel timeval]
	syscall
	cmp rax, 1
	jne fail_d
	mov rax, SYS_WAIT4
	mov rdi, -1
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	puts msg_d, msg_d_len
	jmp do_e

child_d:
	mov eax, [rel pipefd + 4]
	mov edi, eax
	mov rax, SYS_WRITE
	lea rsi, [rel byte_x]
	mov rdx, 1
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

do_e:
	; ---- E: epoll_wait on pipe ----
	mov rax, SYS_PIPE
	lea rdi, [rel pipefd]
	syscall
	test rax, rax
	js fail_e
	mov rax, SYS_EPOLL_CREATE1
	xor rdi, rdi
	syscall
	cmp rax, -4095
	jae fail_e
	mov r12, rax				; epfd
	mov eax, [rel pipefd]
	mov [rel epev], dword EPOLLIN
	mov [rel epev + 4], rax			; data = fd (low)
	mov dword [rel epev + 8], 0
	mov rax, SYS_EPOLL_CTL
	mov rdi, r12
	mov rsi, EPOLL_CTL_ADD
	mov edx, [rel pipefd]
	lea r10, [rel epev]
	syscall
	test rax, rax
	js fail_e
	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail_e
	jz child_e
	mov rax, SYS_EPOLL_WAIT
	mov rdi, r12
	lea rsi, [rel epev]
	mov rdx, 1
	mov r10, 2000
	syscall
	cmp rax, 1
	jne fail_e
	mov eax, [rel epev]
	test eax, EPOLLIN
	jz fail_e
	mov rax, SYS_WAIT4
	mov rdi, -1
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	syscall
	puts msg_e, msg_e_len
	jmp do_f

child_e:
	mov eax, [rel pipefd + 4]
	mov edi, eax
	mov rax, SYS_WRITE
	lea rsi, [rel byte_x]
	mov rdx, 1
	syscall
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

do_f:
	; ---- F: POLLNVAL on bogus fd ----
	mov dword [rel pfd], 99
	mov word [rel pfd + 4], POLLIN
	mov word [rel pfd + 6], 0
	mov rax, SYS_POLL
	lea rdi, [rel pfd]
	mov rsi, 1
	xor rdx, rdx
	syscall
	cmp rax, 1
	jne fail_f
	movzx ebx, word [rel pfd + 6]
	test ebx, POLLNVAL
	jz fail_f
	puts msg_f, msg_f_len

	puts msg_ok, msg_ok_len
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

fail_a:
	puts err_a, err_a_len
	jmp die
fail_b:
	puts err_b, err_b_len
	jmp die
fail_c:
	puts err_c, err_c_len
	jmp die
fail_d:
	puts err_d, err_d_len
	jmp die
fail_e:
	puts err_e, err_e_len
	jmp die
fail_f:
	puts err_f, err_f_len
die:
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

section .bss
pipefd:	resd 2
pfd:	resd 2				; pollfd
fdset:	resq 2
timeval:resq 2
status:	resd 1
epev:	resb 16

section .rodata
byte_x:		db 'x'
msg_a:		db "poll A: stdout POLLOUT OK", 10
msg_a_len equ $ - msg_a
msg_b:		db "poll B: timeout empty pipe OK", 10
msg_b_len equ $ - msg_b
msg_c:		db "poll C: pipe+fork POLLIN OK", 10
msg_c_len equ $ - msg_c
msg_d:		db "select D: pipe+fork OK", 10
msg_d_len equ $ - msg_d
msg_e:		db "epoll E: wait pipe OK", 10
msg_e_len equ $ - msg_e
msg_f:		db "poll F: POLLNVAL OK", 10
msg_f_len equ $ - msg_f
msg_ok:		db "polltest: ALL PASS", 10
msg_ok_len equ $ - msg_ok
err_a:		db "polltest FAIL A", 10
err_a_len equ $ - err_a
err_b:		db "polltest FAIL B", 10
err_b_len equ $ - err_b
err_c:		db "polltest FAIL C", 10
err_c_len equ $ - err_c
err_d:		db "polltest FAIL D", 10
err_d_len equ $ - err_d
err_e:		db "polltest FAIL E", 10
err_e_len equ $ - err_e
err_f:		db "polltest FAIL F", 10
err_f_len equ $ - err_f
