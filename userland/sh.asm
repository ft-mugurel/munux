; munux freestanding /bin/sh (U7)
; Linux x86_64 syscalls: read write open close fork execve wait4 exit chdir getcwd
;
; Builtins: exit, help, cd, pwd
; External: fork + execve("/bin/<cmd>" or path) + wait4
bits 64
section .text
global _start

_start:
	; banner
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_banner]
	mov rdx, msg_banner_len
	syscall

.main_loop:
	; prompt
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_prompt]
	mov rdx, msg_prompt_len
	syscall

	; read line into linebuf
	call read_line
	; r12 = length (no trailing NL)

	test r12, r12
	jz .main_loop			; empty line

	; builtins
	lea rdi, [rel linebuf]
	mov rsi, r12
	call is_exit
	test rax, rax
	jnz .do_exit

	lea rdi, [rel linebuf]
	mov rsi, r12
	call is_help
	test rax, rax
	jnz .do_help

	lea rdi, [rel linebuf]
	mov rsi, r12
	call is_pwd
	test rax, rax
	jnz .do_pwd

	lea rdi, [rel linebuf]
	mov rsi, r12
	call try_cd
	test rax, rax
	jnz .main_loop			; cd handled (ok or error printed)

	; external command: build path, fork/exec/wait
	lea rdi, [rel linebuf]
	mov rsi, r12
	call build_exec_path		; pathbuf NUL-terminated

	; fork
	mov rax, 57
	syscall
	test rax, rax
	js .fork_fail
	jz .child

	; parent: wait4(-1, &status, 0, 0)
	mov r13, rax			; child pid (informational)
	mov rax, 61
	mov rdi, -1
	lea rsi, [rel wait_status]
	xor rdx, rdx
	xor r10, r10
	syscall
	jmp .main_loop

.child:
	; execve(pathbuf, NULL, NULL)
	mov rax, 59
	lea rdi, [rel pathbuf]
	xor rsi, rsi
	xor rdx, rdx
	syscall
	; failed
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_exec_fail]
	mov rdx, msg_exec_fail_len
	syscall
	mov rax, 60
	mov rdi, 127
	syscall

.fork_fail:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_fork_fail]
	mov rdx, msg_fork_fail_len
	syscall
	jmp .main_loop

.do_exit:
	mov rax, 60
	xor rdi, rdi
	syscall

.do_help:
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_help]
	mov rdx, msg_help_len
	syscall
	jmp .main_loop

.do_pwd:
	mov rax, 79			; getcwd
	lea rdi, [rel cwd_buf]
	mov rsi, 256
	syscall
	cmp rax, -4095
	jae .pwd_fail
	mov r12, rax			; length including NUL on Linux; we print without extra
	; print path (rax includes NUL length — print rax-1 or until NUL)
	mov rcx, rax
	test rcx, rcx
	jz .pwd_nl
	dec rcx				; drop NUL from count if present
	jz .pwd_nl
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel cwd_buf]
	mov rdx, rcx
	syscall
.pwd_nl:
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_nl]
	mov rdx, 1
	syscall
	jmp .main_loop
.pwd_fail:
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_pwd_fail]
	mov rdx, msg_pwd_fail_len
	syscall
	jmp .main_loop

; ---------------------------------------------------------------------------
; read_line: fill linebuf, echo chars, stop on NL. r12 = length without NL.
; ---------------------------------------------------------------------------
read_line:
	xor r12, r12
.rl_loop:
	cmp r12, 120
	jae .rl_done
	mov rax, 0			; read
	mov rdi, 0
	lea rsi, [rel onebyte]
	mov rdx, 1
	syscall
	cmp rax, 1
	jne .rl_done
	mov al, [rel onebyte]
	cmp al, 10			; NL
	je .rl_done
	cmp al, 13			; CR → treat as NL
	je .rl_done
	cmp al, 8			; backspace
	je .rl_bs
	cmp al, 127			; DEL
	je .rl_bs
	; store
	lea rdi, [rel linebuf]
	add rdi, r12
	mov [rdi], al
	inc r12
	; echo
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel onebyte]
	mov rdx, 1
	syscall
	jmp .rl_loop
.rl_bs:
	test r12, r12
	jz .rl_loop
	dec r12
	; echo BS space BS
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_bs]
	mov rdx, 3
	syscall
	jmp .rl_loop
.rl_done:
	; echo newline
	mov rax, 1
	mov rdi, 1
	lea rsi, [rel msg_nl]
	mov rdx, 1
	syscall
	; NUL terminate
	lea rdi, [rel linebuf]
	add rdi, r12
	mov byte [rdi], 0
	ret

; ---------------------------------------------------------------------------
; is_exit(rdi=buf, rsi=len) -> rax 1 if "exit"
; ---------------------------------------------------------------------------
is_exit:
	cmp rsi, 4
	jne .no
	cmp byte [rdi], 'e'
	jne .no
	cmp byte [rdi+1], 'x'
	jne .no
	cmp byte [rdi+2], 'i'
	jne .no
	cmp byte [rdi+3], 't'
	jne .no
	mov rax, 1
	ret
.no:
	xor rax, rax
	ret

is_help:
	cmp rsi, 4
	jne .no
	cmp byte [rdi], 'h'
	jne .no
	cmp byte [rdi+1], 'e'
	jne .no
	cmp byte [rdi+2], 'l'
	jne .no
	cmp byte [rdi+3], 'p'
	jne .no
	mov rax, 1
	ret
.no:
	xor rax, rax
	ret

is_pwd:
	cmp rsi, 3
	jne .no
	cmp byte [rdi], 'p'
	jne .no
	cmp byte [rdi+1], 'w'
	jne .no
	cmp byte [rdi+2], 'd'
	jne .no
	mov rax, 1
	ret
.no:
	xor rax, rax
	ret

; try_cd: if line is "cd" or "cd path", chdir. rax=1 if was cd, 0 otherwise
try_cd:
	cmp rsi, 2
	jb .not_cd
	cmp byte [rdi], 'c'
	jne .not_cd
	cmp byte [rdi+1], 'd'
	jne .not_cd
	cmp rsi, 2
	je .cd_home			; bare "cd" → /
	cmp byte [rdi+2], ' '
	jne .not_cd
	; skip spaces
	add rdi, 3
	sub rsi, 3
.skip_sp:
	test rsi, rsi
	jz .cd_home
	cmp byte [rdi], ' '
	jne .cd_path
	inc rdi
	dec rsi
	jmp .skip_sp
.cd_home:
	lea rdi, [rel path_root]
	jmp .cd_do
.cd_path:
	; rdi points at path in linebuf (NUL already at end of line)
	; ensure NUL (already there from read_line)
.cd_do:
	mov rax, 80			; chdir
	; rdi already path
	xor rsi, rsi
	syscall
	cmp rax, -4095
	jb .cd_ok
	mov rax, 1
	mov rdi, 2
	lea rsi, [rel msg_cd_fail]
	mov rdx, msg_cd_fail_len
	syscall
.cd_ok:
	mov rax, 1
	ret
.not_cd:
	xor rax, rax
	ret

; build_exec_path(rdi=cmd, rsi=len) -> pathbuf
; If cmd contains '/', copy as-is; else "/bin/" + cmd
build_exec_path:
	push rbx
	mov rbx, rdi			; cmd
	mov rcx, rsi			; len
	; scan for /
	mov rdx, 0
.scan:
	cmp rdx, rcx
	jae .no_slash
	cmp byte [rbx+rdx], '/'
	je .has_slash
	inc rdx
	jmp .scan
.has_slash:
	; copy cmd to pathbuf
	lea rdi, [rel pathbuf]
	mov rsi, rbx
	mov rdx, rcx
	call memcpy
	lea rdi, [rel pathbuf]
	add rdi, rcx
	mov byte [rdi], 0
	pop rbx
	ret
.no_slash:
	; "/bin/" + cmd
	lea rdi, [rel pathbuf]
	lea rsi, [rel prefix_bin]
	mov rdx, 5
	call memcpy
	lea rdi, [rel pathbuf]
	add rdi, 5
	mov rsi, rbx
	mov rdx, rcx
	call memcpy
	lea rdi, [rel pathbuf]
	add rdi, 5
	add rdi, rcx
	mov byte [rdi], 0
	pop rbx
	ret

; memcpy(rdi=dst, rsi=src, rdx=n)
memcpy:
	test rdx, rdx
	jz .done
.mloop:
	mov al, [rsi]
	mov [rdi], al
	inc rsi
	inc rdi
	dec rdx
	jnz .mloop
.done:
	ret

section .rodata
msg_banner:	db "munux sh (U7). builtins: help exit cd pwd; else fork/exec /bin/<cmd>", 10
msg_banner_len equ $ - msg_banner
msg_prompt:	db "$ "
msg_prompt_len equ $ - msg_prompt
msg_help:	db "help  this text", 10
		db "exit  leave shell", 10
		db "cd [path]  change directory (/ if omitted)", 10
		db "pwd  print working directory", 10
		db "cmd  run /bin/cmd via fork+execve+wait", 10
msg_help_len equ $ - msg_help
msg_nl:		db 10
msg_bs:		db 8, 32, 8
msg_exec_fail:	db "sh: exec failed", 10
msg_exec_fail_len equ $ - msg_exec_fail
msg_fork_fail:	db "sh: fork failed", 10
msg_fork_fail_len equ $ - msg_fork_fail
msg_cd_fail:	db "sh: cd failed", 10
msg_cd_fail_len equ $ - msg_cd_fail
msg_pwd_fail:	db "sh: pwd failed", 10
msg_pwd_fail_len equ $ - msg_pwd_fail
prefix_bin:	db "/bin/"
path_root:	db "/", 0

section .bss
align 16
linebuf:	resb 128
pathbuf:	resb 160
cwd_buf:	resb 256
onebyte:	resb 1
wait_status:	resd 1
