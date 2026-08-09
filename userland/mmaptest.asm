; mmap smoke — anon + file-backed MAP_PRIVATE (offset via r9)
bits 64
section .text
global _start

%define SYS_READ	0
%define SYS_WRITE	1
%define SYS_OPEN	2
%define SYS_CLOSE	3
%define SYS_MMAP	9
%define SYS_MUNMAP	11
%define SYS_EXIT	60

%define PROT_READ	1
%define PROT_WRITE	2
%define MAP_SHARED	1
%define MAP_PRIVATE	2
%define MAP_ANONYMOUS	0x20
%define O_RDONLY	0
%define O_RDWR		2

%macro puts 2
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel %1]
	mov rdx, %2
	syscall
%endmacro

_start:
	; ---- A: anonymous map ----
	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 4096
	mov rdx, PROT_READ | PROT_WRITE
	mov r10, MAP_PRIVATE | MAP_ANONYMOUS
	mov r8, -1
	xor r9, r9
	syscall
	cmp rax, -4095
	jae fail_a
	mov r12, rax
	mov byte [r12], 0x4D
	mov dword [r12 + 4], 0x50414D4D
	cmp byte [r12], 0x4D
	jne fail_a
	cmp dword [r12 + 4], 0x50414D4D
	jne fail_a
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
	cmp rax, -4095
	jae fail_a
	puts msg_a, msg_a_len

	; ---- B: file map /hello.txt at offset 0 ----
	mov rax, SYS_OPEN
	lea rdi, [rel path_hello]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae fail_b
	mov r13, rax			; fd

	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 8192			; 2 pages; file is tiny → rest zero
	mov rdx, PROT_READ | PROT_WRITE
	mov r10, MAP_PRIVATE
	mov r8, r13
	xor r9, r9			; offset 0
	syscall
	cmp rax, -4095
	jae fail_b
	mov r12, rax

	; "Hello from munux ext2!"
	cmp byte [r12], 'H'
	jne fail_b
	cmp byte [r12 + 1], 'e'
	jne fail_b
	cmp byte [r12 + 6], 'f'		; Hello[space]f...
	jne fail_b
	; padding after EOF must stay zero
	cmp byte [r12 + 64], 0
	jne fail_b
	cmp byte [r12 + 4096], 0
	jne fail_b

	; private: write must not require shared file
	mov byte [r12], 'X'
	cmp byte [r12], 'X'
	jne fail_b

	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 8192
	syscall
	cmp rax, -4095
	jae fail_b

	; fd offset must be unchanged — read() still starts at 0
	sub rsp, 16
	mov rax, SYS_READ
	mov rdi, r13
	mov rsi, rsp
	mov rdx, 5
	syscall
	cmp rax, 5
	jne fail_b_stack
	cmp byte [rsp], 'H'
	jne fail_b_stack
	add rsp, 16

	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	puts msg_b, msg_b_len

	; ---- C: unaligned offset → EINVAL (-22) ----
	mov rax, SYS_OPEN
	lea rdi, [rel path_hello]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae fail_c
	mov r13, rax
	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 4096
	mov rdx, PROT_READ
	mov r10, MAP_PRIVATE
	mov r8, r13
	mov r9, 512			; not page-aligned
	syscall
	cmp rax, -22
	jne fail_c_close
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	puts msg_c, msg_c_len

	; ---- D: page-aligned offset past EOF → zeros ----
	mov rax, SYS_OPEN
	lea rdi, [rel path_hello]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae fail_d
	mov r13, rax
	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 4096
	mov rdx, PROT_READ | PROT_WRITE
	mov r10, MAP_PRIVATE
	mov r8, r13
	mov r9, 4096			; past EOF
	syscall
	cmp rax, -4095
	jae fail_d_close
	mov r12, rax
	cmp qword [r12], 0
	jne fail_d_unmap
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	puts msg_d, msg_d_len

	; ---- E: second file /docs/readme.txt ----
	mov rax, SYS_OPEN
	lea rdi, [rel path_readme]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae fail_e
	mov r13, rax
	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 4096
	mov rdx, PROT_READ
	mov r10, MAP_PRIVATE
	mov r8, r13
	xor r9, r9
	syscall
	cmp rax, -4095
	jae fail_e_close
	mov r12, rax
	cmp byte [r12], 'r'		; "readme content"
	jne fail_e_unmap
	cmp byte [r12 + 1], 'e'
	jne fail_e_unmap
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	puts msg_e, msg_e_len

	; ---- F: MAP_SHARED writeback on munmap ----
	mov rax, SYS_OPEN
	lea rdi, [rel path_hello]
	mov rsi, O_RDWR
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae fail_f
	mov r13, rax
	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 4096
	mov rdx, PROT_READ | PROT_WRITE
	mov r10, MAP_SHARED
	mov r8, r13
	xor r9, r9
	syscall
	cmp rax, -4095
	jae fail_f_close
	mov r12, rax
	cmp byte [r12], 'H'
	jne fail_f_unmap
	mov byte [r12], 'S'
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
	cmp rax, -4095
	jae fail_f_close
	; fd offset unchanged; read first byte from file
	sub rsp, 16
	mov rax, SYS_READ
	mov rdi, r13
	mov rsi, rsp
	mov rdx, 1
	syscall
	cmp rax, 1
	jne fail_f_stack
	cmp byte [rsp], 'S'
	jne fail_f_stack
	add rsp, 16
	; restore 'H' so other tests still see the original file
	mov rax, SYS_MMAP
	xor rdi, rdi
	mov rsi, 4096
	mov rdx, PROT_READ | PROT_WRITE
	mov r10, MAP_SHARED
	mov r8, r13
	xor r9, r9
	syscall
	cmp rax, -4095
	jae fail_f_close
	mov r12, rax
	mov byte [r12], 'H'
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	puts msg_f, msg_f_len

	puts msg_ok, msg_ok_len
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall

fail_f_stack:
	add rsp, 16
	jmp fail_f
fail_f_unmap:
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
fail_f_close:
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	jmp fail_f

fail_b_stack:
	add rsp, 16
	jmp fail_b
fail_c_close:
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	jmp fail_c
fail_d_unmap:
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
fail_d_close:
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	jmp fail_d
fail_e_unmap:
	mov rax, SYS_MUNMAP
	mov rdi, r12
	mov rsi, 4096
	syscall
fail_e_close:
	mov rax, SYS_CLOSE
	mov rdi, r13
	syscall
	jmp fail_e

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

section .rodata
path_hello:	db "/hello.txt", 0
path_readme:	db "/docs/readme.txt", 0
msg_a:		db "mmap A: anon OK", 10
msg_a_len equ $ - msg_a
msg_b:		db "mmap B: file /hello.txt OK (pos preserved)", 10
msg_b_len equ $ - msg_b
msg_c:		db "mmap C: offset 512 -> EINVAL OK", 10
msg_c_len equ $ - msg_c
msg_d:		db "mmap D: offset 4096 past EOF zeros OK", 10
msg_d_len equ $ - msg_d
msg_e:		db "mmap E: file /docs/readme.txt OK", 10
msg_e_len equ $ - msg_e
msg_f:		db "mmap F: MAP_SHARED writeback OK", 10
msg_f_len equ $ - msg_f
msg_ok:		db "mmaptest: ALL PASS", 10
msg_ok_len equ $ - msg_ok
err_a:		db "mmaptest FAIL A anon", 10
err_a_len equ $ - err_a
err_b:		db "mmaptest FAIL B file", 10
err_b_len equ $ - err_b
err_c:		db "mmaptest FAIL C align", 10
err_c_len equ $ - err_c
err_d:		db "mmaptest FAIL D eof", 10
err_d_len equ $ - err_d
err_e:		db "mmaptest FAIL E readme", 10
err_e_len equ $ - err_e
err_f:		db "mmaptest FAIL F shared", 10
err_f_len equ $ - err_f
