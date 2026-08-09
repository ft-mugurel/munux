; P9d: prctl + execveat smoke
bits 64
section .text
global _start

%define SYS_WRITE		1
%define SYS_OPEN		2
%define SYS_CLOSE		3
%define SYS_FORK		57
%define SYS_EXIT		60
%define SYS_WAIT4		61
%define SYS_PRCTL		157
%define SYS_EXECVEAT		322

%define AT_FDCWD		-100
%define AT_EMPTY_PATH		0x1000
%define O_RDONLY		0
%define O_DIRECTORY		0x10000

%define PR_SET_PDEATHSIG	1
%define PR_GET_PDEATHSIG	2
%define PR_GET_DUMPABLE		3
%define PR_SET_DUMPABLE		4
%define PR_SET_NAME		15
%define PR_GET_NAME		16
%define PR_SET_NO_NEW_PRIVS	38
%define PR_GET_NO_NEW_PRIVS	39

%macro puts 2
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel %1]
	mov rdx, %2
	syscall
%endmacro

_start:
	; ---- A: PR_SET_NAME / PR_GET_NAME ----
	mov rax, SYS_PRCTL
	mov rdi, PR_SET_NAME
	lea rsi, [rel comm_set]
	syscall
	cmp rax, -4095
	jae fail_a
	mov rax, SYS_PRCTL
	mov rdi, PR_GET_NAME
	lea rsi, [rel comm_got]
	syscall
	cmp rax, -4095
	jae fail_a
	lea rsi, [rel comm_set]
	lea rdi, [rel comm_got]
	mov rcx, 6			; "p9comm"
.cmp_a:
	mov al, [rsi]
	mov bl, [rdi]
	cmp al, bl
	jne fail_a
	inc rsi
	inc rdi
	loop .cmp_a
	cmp byte [rel comm_got + 6], 0
	jne fail_a
	puts msg_a, msg_a_len

	; ---- B: dumpable 0 then 1 ----
	mov rax, SYS_PRCTL
	mov rdi, PR_SET_DUMPABLE
	xor rsi, rsi
	syscall
	test rax, rax
	jnz fail_b
	mov rax, SYS_PRCTL
	mov rdi, PR_GET_DUMPABLE
	syscall
	test rax, rax
	jnz fail_b
	mov rax, SYS_PRCTL
	mov rdi, PR_SET_DUMPABLE
	mov rsi, 1
	syscall
	test rax, rax
	jnz fail_b
	mov rax, SYS_PRCTL
	mov rdi, PR_GET_DUMPABLE
	syscall
	cmp rax, 1
	jne fail_b
	puts msg_b, msg_b_len

	; ---- C: no_new_privs sticky ----
	mov rax, SYS_PRCTL
	mov rdi, PR_SET_NO_NEW_PRIVS
	mov rsi, 1
	syscall
	test rax, rax
	jnz fail_c
	mov rax, SYS_PRCTL
	mov rdi, PR_GET_NO_NEW_PRIVS
	syscall
	cmp rax, 1
	jne fail_c
	mov rax, SYS_PRCTL
	mov rdi, PR_SET_NO_NEW_PRIVS
	xor rsi, rsi			; must stay 1; Linux EINVAL
	syscall
	cmp rax, -22
	jne fail_c
	mov rax, SYS_PRCTL
	mov rdi, PR_GET_NO_NEW_PRIVS
	syscall
	cmp rax, 1
	jne fail_c
	puts msg_c, msg_c_len

	; ---- D: unknown option → EINVAL; pdeathsig SET/GET ----
	mov rax, SYS_PRCTL
	mov rdi, 999
	xor rsi, rsi
	syscall
	cmp rax, -22
	jne fail_d
	mov rax, SYS_PRCTL
	mov rdi, PR_SET_PDEATHSIG
	mov rsi, 15			; SIGTERM
	syscall
	test rax, rax
	jnz fail_d
	mov dword [rel pdsig], 0
	mov rax, SYS_PRCTL
	mov rdi, PR_GET_PDEATHSIG
	lea rsi, [rel pdsig]
	syscall
	test rax, rax
	jnz fail_d
	cmp dword [rel pdsig], 15
	jne fail_d
	; Clear so later wait/exec is not coupled to PDEATHSIG (Linux: not inherited
	; by children, but keep this process inert for the rest of the smoke).
	mov rax, SYS_PRCTL
	mov rdi, PR_SET_PDEATHSIG
	xor rsi, rsi
	syscall
	test rax, rax
	jnz fail_d
	puts msg_d, msg_d_len

	; ---- E: execveat(AT_FDCWD, "/bin/hello", …) ----
	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail_e
	jz .child_e
	mov rdi, rax
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	mov rax, SYS_WAIT4
	syscall
	cmp rax, -4095
	jae fail_e
	cmp dword [rel status], 0
	jne fail_e
	puts msg_e, msg_e_len
	jmp .do_f
.child_e:
	mov rax, SYS_EXECVEAT
	mov rdi, AT_FDCWD
	lea rsi, [rel path_hello]
	xor rdx, rdx
	xor r10, r10
	xor r8, r8
	syscall
	mov rax, SYS_EXIT
	mov rdi, 77
	syscall

.do_f:
	; ---- F: fexecve = execveat(fd, "", AT_EMPTY_PATH) ----
	mov rax, SYS_OPEN
	lea rdi, [rel path_hello]
	xor rsi, rsi
	syscall
	cmp rax, -4095
	jae fail_f
	mov [rel saved_fd], eax		; fork only restores rip/rsp/rax
	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail_f
	jz .child_f
	mov rdi, rax
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	mov rax, SYS_WAIT4
	syscall
	cmp rax, -4095
	jae fail_f
	cmp dword [rel status], 0
	jne fail_f
	mov edi, [rel saved_fd]
	mov rax, SYS_CLOSE
	syscall
	puts msg_f, msg_f_len
	jmp .do_g
.child_f:
	mov rax, SYS_EXECVEAT
	mov edi, [rel saved_fd]
	lea rsi, [rel empty_path]
	xor rdx, rdx
	xor r10, r10
	mov r8, AT_EMPTY_PATH
	syscall
	mov rax, SYS_EXIT
	mov rdi, 77
	syscall

.do_g:
	; ---- G: execveat(dirfd=/bin, "hello") ----
	mov rax, SYS_OPEN
	lea rdi, [rel path_bindir]
	mov rsi, O_RDONLY | O_DIRECTORY
	syscall
	cmp rax, -4095
	jae fail_g
	mov [rel saved_fd], eax
	mov rax, SYS_FORK
	syscall
	test rax, rax
	js fail_g
	jz .child_g
	mov rdi, rax
	lea rsi, [rel status]
	xor rdx, rdx
	xor r10, r10
	mov rax, SYS_WAIT4
	syscall
	cmp rax, -4095
	jae fail_g
	cmp dword [rel status], 0
	jne fail_g
	mov edi, [rel saved_fd]
	mov rax, SYS_CLOSE
	syscall
	puts msg_g, msg_g_len
	jmp .do_h
.child_g:
	mov rax, SYS_EXECVEAT
	mov edi, [rel saved_fd]
	lea rsi, [rel rel_hello]
	xor rdx, rdx
	xor r10, r10
	xor r8, r8
	syscall
	mov rax, SYS_EXIT
	mov rdi, 77
	syscall

.do_h:
	; ---- H: execveat unknown flags → EINVAL ----
	mov rax, SYS_EXECVEAT
	mov rdi, AT_FDCWD
	lea rsi, [rel path_hello]
	xor rdx, rdx
	xor r10, r10
	mov r8, 2			; bogus flag
	syscall
	cmp rax, -22
	jne fail_h
	puts msg_h, msg_h_len

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
	jmp die
fail_g:
	puts err_g, err_g_len
	jmp die
fail_h:
	puts err_h, err_h_len
die:
	mov rax, SYS_EXIT
	mov rdi, 1
	syscall

section .bss
comm_got:	resb 16
pdsig:		resd 1
status:		resd 1
saved_fd:	resd 1

section .rodata
comm_set:	db "p9comm", 0
path_hello:	db "/bin/hello", 0
path_bindir:	db "/bin", 0
rel_hello:	db "hello", 0
empty_path:	db 0
msg_a:		db "p9 A: prctl name OK", 10
msg_a_len equ $ - msg_a
msg_b:		db "p9 B: dumpable OK", 10
msg_b_len equ $ - msg_b
msg_c:		db "p9 C: no_new_privs OK", 10
msg_c_len equ $ - msg_c
msg_d:		db "p9 D: EINVAL+pdeathsig OK", 10
msg_d_len equ $ - msg_d
msg_e:		db "p9 E: execveat AT_FDCWD OK", 10
msg_e_len equ $ - msg_e
msg_f:		db "p9 F: execveat AT_EMPTY_PATH OK", 10
msg_f_len equ $ - msg_f
msg_g:		db "p9 G: execveat dirfd relative OK", 10
msg_g_len equ $ - msg_g
msg_h:		db "p9 H: execveat bad flags EINVAL OK", 10
msg_h_len equ $ - msg_h
msg_ok:		db "p9test: ALL PASS", 10
msg_ok_len equ $ - msg_ok
err_a:		db "p9test FAIL A", 10
err_a_len equ $ - err_a
err_b:		db "p9test FAIL B", 10
err_b_len equ $ - err_b
err_c:		db "p9test FAIL C", 10
err_c_len equ $ - err_c
err_d:		db "p9test FAIL D", 10
err_d_len equ $ - err_d
err_e:		db "p9test FAIL E", 10
err_e_len equ $ - err_e
err_f:		db "p9test FAIL F", 10
err_f_len equ $ - err_f
err_g:		db "p9test FAIL G", 10
err_g_len equ $ - err_g
err_h:		db "p9test FAIL H", 10
err_h_len equ $ - err_h
