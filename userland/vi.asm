; munux mini-vi — freestanding vim-like editor (userspace)
; Modes: NORMAL / INSERT / COMMAND (:)
; NORMAL: hjkl 0 $ w b x i a o dd :  arrows
; INSERT: type, BS, Enter, Esc
; COMMAND: :w :q :wq :q!  Enter
;
; Linux x86_64: read=0 write=1 open=2 close=3 exit=60
; open flags: O_RDONLY=0 O_WRONLY=1 O_RDWR=2 O_CREAT=64 O_TRUNC=512
bits 64
section .text
global _start

%define BUF_MAX		4096
%define PATH_MAX	96
%define CMD_MAX		64
%define VIEW_ROWS	22
%define COLS		80

%define MODE_NORMAL	0
%define MODE_INSERT	1
%define MODE_CMD	2

%define SYS_READ	0
%define SYS_WRITE	1
%define SYS_OPEN	2
%define SYS_CLOSE	3
%define SYS_EXIT	60

%define O_RDONLY	0
%define O_WRONLY	1
%define O_CREAT		64
%define O_TRUNC		512
%define O_CREAT_TRUNC	577		; O_WRONLY|O_CREAT|O_TRUNC = 1|64|512

_start:
	; argc / argv[1]
	mov rax, [rsp]
	cmp rax, 2
	jb .no_arg
	mov rsi, [rsp+16]
	test rsi, rsi
	jz .no_arg
	lea rdi, [rel pathbuf]
	call strcpy
	jmp .have_path
.no_arg:
	lea rsi, [rel default_path]
	lea rdi, [rel pathbuf]
	call strcpy
.have_path:
	call load_file
	mov byte [rel mode], MODE_NORMAL
	mov dword [rel dirty], 0
	mov dword [rel cursor], 0
	mov dword [rel row0], 0
	mov dword [rel pref_col], 0

.main:
	call redraw
	call read_key			; al = key
	movzx eax, al
	movzx ebx, byte [rel mode]
	cmp ebx, MODE_INSERT
	je .do_insert
	cmp ebx, MODE_CMD
	je .do_cmd
	; NORMAL
	call handle_normal
	jmp .main
.do_insert:
	call handle_insert
	jmp .main
.do_cmd:
	call handle_cmd
	jmp .main

; ---- key handlers ----
; al = key
handle_normal:
	cmp al, 0x1B			; Esc — stay normal
	je .ret
	cmp al, 0x80			; up
	je .kup
	cmp al, 0x81
	je .kdown
	cmp al, 0x82
	je .kright
	cmp al, 0x83
	je .kleft
	cmp al, 'h'
	je .kleft
	cmp al, 'l'
	je .kright
	cmp al, 'j'
	je .kdown
	cmp al, 'k'
	je .kup
	cmp al, '0'
	je .khome
	cmp al, '$'
	je .kend
	cmp al, 'i'
	je .kins
	cmp al, 'a'
	je .kapp
	cmp al, 'o'
	je .kopen
	cmp al, 'x'
	je .kdel
	cmp al, 'd'
	je .kdd
	cmp al, ':'
	je .kcolon
	cmp al, 10
	je .kdown
	ret
.kup:
	call cur_up
	ret
.kdown:
	call cur_down
	ret
.kleft:
	call cur_left
	ret
.kright:
	call cur_right
	ret
.khome:
	call cur_line_start
	ret
.kend:
	call cur_line_end
	ret
.kins:
	mov byte [rel mode], MODE_INSERT
	ret
.kapp:
	; append after current char (if not at EOL NL)
	mov eax, [rel cursor]
	mov ecx, [rel buflen]
	cmp eax, ecx
	jae .a_ins
	lea rsi, [rel buffer]
	cmp byte [rsi+rax], 10
	je .a_ins
	inc eax
	mov [rel cursor], eax
.a_ins:
	mov byte [rel mode], MODE_INSERT
	ret
.kopen:
	; vim `o`: new line below current, enter insert
	mov edi, [rel cursor]
	call line_end_of			; index of NL or buflen
	mov [rel cursor], eax
	mov al, 10
	call buf_insert			; insert NL at EOL → blank line below
	mov byte [rel mode], MODE_INSERT
	ret
.kdel:
	call buf_delete
	ret
.kdd:
	; second 'd' expected — simple: delete whole line now
	call delete_line
	ret
.kcolon:
	mov byte [rel mode], MODE_CMD
	mov dword [rel cmdlen], 0
	mov byte [rel cmdbuf], 0
	ret
.ret:
	ret

handle_insert:
	cmp al, 0x1B
	je .esc
	cmp al, 0x08
	je .bs
	cmp al, 0x7F
	je .bs
	cmp al, 10
	je .nl
	cmp al, 0x80
	jb .maybe
	cmp al, 0x83
	ja .maybe
	; arrows move in insert too
	cmp al, 0x80
	je .iu
	cmp al, 0x81
	je .id
	cmp al, 0x82
	je .ir
	cmp al, 0x83
	je .il
.maybe:
	cmp al, 0x20
	jb .done
	cmp al, 0x7F
	jae .done
	call buf_insert
	ret
.bs:
	call cur_left_if
	call buf_delete
	ret
.nl:
	mov al, 10
	call buf_insert
	ret
.esc:
	mov byte [rel mode], MODE_NORMAL
	; leave cursor on last typed char (vim-like)
	mov eax, [rel cursor]
	test eax, eax
	jz .done
	dec eax
	mov [rel cursor], eax
	ret
.iu:
	call cur_up
	ret
.id:
	call cur_down
	ret
.ir:
	call cur_right
	ret
.il:
	call cur_left
	ret
.done:
	ret

handle_cmd:
	cmp al, 0x1B
	je .cancel
	cmp al, 10
	je .run
	cmp al, 0x08
	je .bs
	cmp al, 0x7F
	je .bs
	cmp al, 0x20
	jb .done
	cmp al, 0x7F
	jae .done
	mov ecx, [rel cmdlen]
	cmp ecx, CMD_MAX-1
	jae .done
	lea rdi, [rel cmdbuf]
	add rdi, rcx
	mov [rdi], al
	inc ecx
	mov [rel cmdlen], ecx
	lea rdi, [rel cmdbuf]
	add rdi, rcx
	mov byte [rdi], 0
	ret
.bs:
	mov ecx, [rel cmdlen]
	test ecx, ecx
	jz .done
	dec ecx
	mov [rel cmdlen], ecx
	lea rdi, [rel cmdbuf]
	add rdi, rcx
	mov byte [rdi], 0
	ret
.cancel:
	mov byte [rel mode], MODE_NORMAL
	ret
.run:
	call run_command
	ret
.done:
	ret

; ---- : commands ----
run_command:
	lea rsi, [rel cmdbuf]
	; skip leading spaces
.sk:
	mov al, [rsi]
	cmp al, ' '
	jne .c0
	inc rsi
	jmp .sk
.c0:
	; empty
	cmp al, 0
	je .back
	; q!
	cmp word [rsi], 'q' | ('!' << 8)
	je .quit
	; wq
	cmp word [rsi], 'w' | ('q' << 8)
	je .wq
	; w
	cmp al, 'w'
	jne .notw
	cmp byte [rsi+1], 0
	je .write
	cmp byte [rsi+1], ' '
	je .write
.notw:
	; q
	cmp al, 'q'
	jne .err
	cmp byte [rsi+1], 0
	jne .err
	cmp dword [rel dirty], 0
	jne .err_dirty
	jmp .quit
.write:
	call save_file
	jc .err_save
	mov byte [rel mode], MODE_NORMAL
	lea rsi, [rel msg_written]
	call set_status
	ret
.wq:
	call save_file
	jc .err_save
	jmp .quit
.quit:
	call clear_screen
	mov rax, SYS_EXIT
	xor rdi, rdi
	syscall
.err_dirty:
	lea rsi, [rel msg_dirty]
	call set_status
	mov byte [rel mode], MODE_NORMAL
	ret
.err_save:
	lea rsi, [rel msg_write_fail]
	call set_status
	mov byte [rel mode], MODE_NORMAL
	ret
.err:
	lea rsi, [rel msg_bad_cmd]
	call set_status
.back:
	mov byte [rel mode], MODE_NORMAL
	ret

; ---- buffer ops ----
; insert al at cursor
buf_insert:
	mov ecx, [rel buflen]
	cmp ecx, BUF_MAX-1
	jae .full
	mov edx, [rel cursor]
	; shift right [cursor..len)
	lea rdi, [rel buffer]
	add rdi, rcx			; end
	lea rsi, [rel buffer]
	add rsi, rdx
.shift:
	cmp rdi, rsi
	jbe .place
	mov bl, [rdi-1]
	mov [rdi], bl
	dec rdi
	jmp .shift
.place:
	lea rdi, [rel buffer]
	add rdi, rdx
	mov [rdi], al
	inc ecx
	mov [rel buflen], ecx
	inc edx
	mov [rel cursor], edx
	mov dword [rel dirty], 1
	; update preferred column after insert
	push rax
	call col_at_cursor
	mov [rel pref_col], eax
	pop rax
.full:
	ret

buf_delete:
	mov ecx, [rel buflen]
	mov edx, [rel cursor]
	cmp edx, ecx
	jae .ret
	; shift left
	lea rsi, [rel buffer]
	add rsi, rdx
	inc rsi
	lea rdi, [rel buffer]
	add rdi, rdx
	mov eax, ecx
	sub eax, edx
	dec eax				; count after cursor
	mov ecx, eax
	jz .just_dec
	call memcpy
.just_dec:
	mov ecx, [rel buflen]
	dec ecx
	mov [rel buflen], ecx
	lea rdi, [rel buffer]
	add rdi, rcx
	mov byte [rdi], 0
	mov dword [rel dirty], 1
.ret:
	ret

delete_line:
	; move to line start
	call cur_line_start
	mov edx, [rel cursor]
.dl:
	mov ecx, [rel buflen]
	cmp edx, ecx
	jae .done
	lea rsi, [rel buffer]
	mov al, [rsi+rdx]
	; delete this char
	mov [rel cursor], edx
	push rdx
	call buf_delete
	pop rdx
	cmp al, 10
	je .done
	jmp .dl
.done:
	mov dword [rel dirty], 1
	ret

; ---- cursor ----
; pref_col: column remembered for j/k (vim-style)
; col_at_cursor: eax = column (0-based) of current cursor
col_at_cursor:
	push rbx
	mov eax, [rel cursor]
	lea rsi, [rel buffer]
	xor ebx, ebx			; col
.lp:
	test eax, eax
	jz .done
	cmp byte [rsi+rax-1], 10
	je .done
	dec eax
	inc ebx
	jmp .lp
.done:
	mov eax, ebx
	pop rbx
	ret

; line_start_of: edi = pos → eax = start of that line
line_start_of:
	mov eax, edi
	lea rsi, [rel buffer]
.lp:
	test eax, eax
	jz .d
	cmp byte [rsi+rax-1], 10
	je .d
	dec eax
	jmp .lp
.d:	ret

; line_end_of: edi = pos → eax = index of NL or buflen (not past end)
line_end_of:
	mov eax, edi
	mov ecx, [rel buflen]
	lea rsi, [rel buffer]
.lp:
	cmp eax, ecx
	jae .d
	cmp byte [rsi+rax], 10
	je .d
	inc eax
	jmp .lp
.d:	ret

; goto_col: line starts at edi, want column pref_col → set cursor
goto_col_on_line:
	mov eax, edi			; start
	mov ecx, [rel buflen]
	mov edx, [rel pref_col]
	lea rsi, [rel buffer]
	xor ebx, ebx			; col
.lp:
	cmp eax, ecx
	jae .set
	cmp byte [rsi+rax], 10
	je .set				; stop at NL (cursor on NL like vim $ behavior for short lines)
	cmp ebx, edx
	jae .set
	inc eax
	inc ebx
	jmp .lp
.set:
	mov [rel cursor], eax
	ret

cur_left:
	mov eax, [rel cursor]
	test eax, eax
	jz .r
	dec eax
	mov [rel cursor], eax
	call col_at_cursor
	mov [rel pref_col], eax
.r:	ret

cur_left_if:
	mov eax, [rel cursor]
	test eax, eax
	jz .r
	dec eax
	mov [rel cursor], eax
.r:	ret

cur_right:
	mov eax, [rel cursor]
	mov ecx, [rel buflen]
	cmp eax, ecx
	jae .r
	; don't skip past end; allow landing on NL then past it
	inc eax
	mov [rel cursor], eax
	call col_at_cursor
	mov [rel pref_col], eax
.r:	ret

cur_line_start:
	mov edi, [rel cursor]
	call line_start_of
	mov [rel cursor], eax
	mov dword [rel pref_col], 0
	ret

cur_line_end:
	mov edi, [rel cursor]
	call line_end_of
	; sit on last char of line (or on empty line start); if line has content ending before NL, end is NL index
	; vim $ goes to last non-NL char
	mov ecx, [rel buflen]
	cmp eax, ecx
	jae .set
	lea rsi, [rel buffer]
	cmp byte [rsi+rax], 10
	jne .set
	; eax points at NL — if not at line start, step back one
	mov edi, [rel cursor]
	push rax
	call line_start_of
	pop rdx			; NL index
	cmp eax, edx
	je .use_nl		; empty line: stay on NL/start
	lea eax, [rdx-1]
	jmp .set
.use_nl:
	mov eax, edx
.set:
	mov [rel cursor], eax
	call col_at_cursor
	mov [rel pref_col], eax
	ret

; j / down-arrow: next line, same preferred column
cur_down:
	mov edi, [rel cursor]
	call line_end_of		; eax = NL or buflen
	mov ecx, [rel buflen]
	cmp eax, ecx
	jae .r				; no newline → no next line
	; must be on NL
	lea rsi, [rel buffer]
	cmp byte [rsi+rax], 10
	jne .r
	inc eax				; first byte of next line (may be == buflen)
	cmp eax, ecx
	ja .r
	mov edi, eax			; start of next line
	; refresh pref_col from current if never set mid-line move — keep pref_col
	call goto_col_on_line
.r:	ret

; k / up-arrow: previous line, same preferred column
cur_up:
	mov edi, [rel cursor]
	call line_start_of
	test eax, eax
	jz .r				; already first line
	dec eax				; last char of previous line (or NL)
	mov edi, eax
	call line_start_of
	mov edi, eax
	call goto_col_on_line
.r:	ret

; ---- file I/O ----
load_file:
	; open O_RDONLY
	mov rax, SYS_OPEN
	lea rdi, [rel pathbuf]
	mov rsi, O_RDONLY
	xor rdx, rdx
	syscall
	cmp rax, -4095
	jae .empty
	mov r12, rax
	; read into buffer
	mov rax, SYS_READ
	mov rdi, r12
	lea rsi, [rel buffer]
	mov rdx, BUF_MAX-1
	syscall
	cmp rax, -4095
	jae .close_empty
	mov [rel buflen], eax
	lea rdi, [rel buffer]
	add rdi, rax
	mov byte [rdi], 0
	mov rax, SYS_CLOSE
	mov rdi, r12
	syscall
	mov dword [rel dirty], 0
	lea rsi, [rel msg_opened]
	call set_status
	ret
.close_empty:
	mov rax, SYS_CLOSE
	mov rdi, r12
	syscall
.empty:
	mov dword [rel buflen], 0
	mov byte [rel buffer], 0
	mov dword [rel dirty], 0
	lea rsi, [rel msg_new]
	call set_status
	ret

save_file:
	; open O_WRONLY|O_CREAT|O_TRUNC
	mov rax, SYS_OPEN
	lea rdi, [rel pathbuf]
	mov rsi, O_CREAT_TRUNC
	mov rdx, 0o644
	syscall
	cmp rax, -4095
	jae .fail
	mov r12, rax
	mov rax, SYS_WRITE
	mov rdi, r12
	lea rsi, [rel buffer]
	mov edx, [rel buflen]
	syscall
	cmp rax, -4095
	jae .fail_close
	mov r13, rax
	mov rax, SYS_CLOSE
	mov rdi, r12
	syscall
	cmp r13d, [rel buflen]
	jne .fail
	mov dword [rel dirty], 0
	clc
	ret
.fail_close:
	mov rax, SYS_CLOSE
	mov rdi, r12
	syscall
.fail:
	stc
	ret

; ---- display ----
clear_screen:
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel form_feed]
	mov rdx, 1
	syscall
	ret

redraw:
	call clear_screen
	; compute line of cursor for scroll
	call cursor_line_col		; r8=line r9=col
	mov eax, [rel row0]
	cmp r8d, eax
	jae .ok_top
	mov [rel row0], r8d
	jmp .draw
.ok_top:
	mov ebx, eax
	add ebx, VIEW_ROWS
	cmp r8d, ebx
	jb .draw
	mov eax, r8d
	sub eax, VIEW_ROWS-1
	mov [rel row0], eax
.draw:
	; walk buffer printing VIEW_ROWS lines starting at row0
	xor r10d, r10d			; current line idx
	xor r11d, r11d			; buf index
	mov r12d, [rel buflen]
	mov r13d, [rel row0]
	xor r14d, r14d			; rows printed
.scan:
	cmp r11d, r12d
	jae .pad
	cmp r14d, VIEW_ROWS
	jae .status
	; if current line >= row0, print
	cmp r10d, r13d
	jb .skip_line
	; print line until NL or end; mark cursor
	call print_view_line		; uses r11, updates r11; r10 is line
	inc r14d
	inc r10d
	jmp .scan
.skip_line:
	; advance r11 to next line
	lea rsi, [rel buffer]
.skl:
	cmp r11d, r12d
	jae .scan
	mov al, [rsi+r11]
	inc r11d
	cmp al, 10
	jne .skl
	inc r10d
	jmp .scan
.pad:
	cmp r14d, VIEW_ROWS
	jae .status
	; empty line marker
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel tilde_nl]
	mov rdx, 2
	syscall
	inc r14d
	jmp .pad
.status:
	; status line
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel status_bar]
	mov rdx, 1			; newline before status
	; actually print mode + path + dirty
	lea rsi, [rel msg_nl]
	mov rdx, 1
	syscall
	movzx eax, byte [rel mode]
	cmp eax, MODE_INSERT
	je .st_ins
	cmp eax, MODE_CMD
	je .st_cmd
	lea rsi, [rel st_normal]
	mov rdx, st_normal_len
	jmp .st_w
.st_ins:
	lea rsi, [rel st_insert]
	mov rdx, st_insert_len
	jmp .st_w
.st_cmd:
	lea rsi, [rel st_cmd]
	mov rdx, st_cmd_len
.st_w:
	mov rax, SYS_WRITE
	mov rdi, 1
	syscall
	; path
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel pathbuf]
	call strlen
	mov rdx, rax
	lea rsi, [rel pathbuf]
	mov rax, SYS_WRITE
	mov rdi, 1
	syscall
	cmp dword [rel dirty], 0
	je .st2
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel st_mod]
	mov rdx, st_mod_len
	syscall
.st2:
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_nl]
	mov rdx, 1
	syscall
	; command line if CMD
	cmp byte [rel mode], MODE_CMD
	jne .st3
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel colon]
	mov rdx, 1
	syscall
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel cmdbuf]
	mov edx, [rel cmdlen]
	syscall
	jmp .st_done
.st3:
	; status message
	lea rsi, [rel status_msg]
	cmp byte [rsi], 0
	je .st_done
	mov rax, SYS_WRITE
	mov rdi, 1
	call strlen
	mov rdx, rax
	lea rsi, [rel status_msg]
	mov rax, SYS_WRITE
	mov rdi, 1
	syscall
	mov byte [rel status_msg], 0
.st_done:
	ret

; write_inv_char: al = char (or space). Uses inverse video (0x0E … 0x0F).
write_inv_char:
	push rax
	mov [rel onebyte], al
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel inv_on]
	mov rdx, 1
	syscall
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel onebyte]
	mov rdx, 1
	syscall
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel inv_off]
	mov rdx, 1
	syscall
	pop rax
	ret

; write_plain: al = char
write_plain:
	mov [rel onebyte], al
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel onebyte]
	mov rdx, 1
	syscall
	ret

; print one view line starting at r11; highlight cursor; advance r11 past NL
print_view_line:
	push r12
	push r13
	push rbx
	mov r12d, [rel buflen]
	mov r13d, [rel cursor]
	lea rbx, [rel buffer]
	xor ecx, ecx			; col
.pl:
	cmp r11d, r12d
	jae .eol_pad
	mov al, [rbx+r11]
	cmp al, 10
	je .at_nl
	cmp ecx, COLS-1
	ja .skip_overflow
	; if this index is the cursor, inverse
	cmp r11d, r13d
	jne .plain
	push rcx
	push r11
	call write_inv_char
	pop r11
	pop rcx
	jmp .adv
.plain:
	push rcx
	push r11
	call write_plain
	pop r11
	pop rcx
.adv:
	inc ecx
.skip_overflow:
	inc r11d
	jmp .pl
.at_nl:
	; cursor sitting on NL → show inverse space at end of line
	cmp r11d, r13d
	jne .nl_out
	cmp ecx, COLS-1
	ja .nl_skip
	push rcx
	push r11
	mov al, ' '
	call write_inv_char
	pop r11
	pop rcx
.nl_skip:
	inc r11d				; consume NL
	jmp .nl_out
.eol_pad:
	; EOF / end of buffer on this line
	cmp r11d, r13d
	jne .nl_out
	cmp ecx, COLS-1
	ja .nl_out
	push rcx
	mov al, ' '
	call write_inv_char
	pop rcx
.nl_out:
	mov rax, SYS_WRITE
	mov rdi, 1
	lea rsi, [rel msg_nl]
	mov rdx, 1
	syscall
	pop rbx
	pop r13
	pop r12
	ret

; r8 = line number of cursor, r9 = col
cursor_line_col:
	xor r8d, r8d
	xor r9d, r9d
	xor ecx, ecx
	mov edx, [rel cursor]
	lea rsi, [rel buffer]
.cl:
	cmp ecx, edx
	jae .done
	mov al, [rsi+rcx]
	inc ecx
	cmp al, 10
	jne .col
	inc r8d
	xor r9d, r9d
	jmp .cl
.col:
	inc r9d
	jmp .cl
.done:
	ret

set_status:
	; rsi = C string → status_msg
	lea rdi, [rel status_msg]
	call strcpy
	ret

read_key:
	mov rax, SYS_READ
	mov rdi, 0
	lea rsi, [rel onebyte]
	mov rdx, 1
	syscall
	cmp rax, 1
	jne .zero
	mov al, [rel onebyte]
	ret
.zero:
	xor eax, eax
	ret

; ---- libc-ish ----
strcpy:
	; rdi=dst rsi=src
	push rdi
.lp:
	mov al, [rsi]
	mov [rdi], al
	inc rsi
	inc rdi
	test al, al
	jnz .lp
	pop rdi
	ret

strlen:
	; rsi=str → rax len
	xor eax, eax
.lp:
	cmp byte [rsi+rax], 0
	je .d
	inc eax
	jmp .lp
.d:	ret

memcpy:
	; rdi=dst rsi=src rcx=n
	test rcx, rcx
	jz .d
.lp:
	mov al, [rsi]
	mov [rdi], al
	inc rsi
	inc rdi
	dec rcx
	jnz .lp
.d:	ret

section .rodata
default_path:	db "untitled.txt", 0
form_feed:	db 12
msg_nl:		db 10
tilde_nl:	db '~', 10
colon:		db ':'
inv_on:		db 0x0E			; console: inverse on
inv_off:	db 0x0F			; console: inverse off
st_normal:	db "NORMAL  ",
st_normal_len equ $ - st_normal
st_insert:	db "-- INSERT --  ", 
st_insert_len equ $ - st_insert
st_cmd:		db ":", 
st_cmd_len equ $ - st_cmd
st_mod:		db " [+]", 
st_mod_len equ $ - st_mod
msg_opened:	db "opened", 0
msg_new:	db "new file", 0
msg_written:	db "written", 0
msg_write_fail:	db "write failed", 0
msg_dirty:	db "no write since last change (:q! to quit)", 0
msg_bad_cmd:	db "not an editor command", 0
status_bar:	db 10

section .bss
align 16
buffer:		resb BUF_MAX
pathbuf:	resb PATH_MAX
cmdbuf:		resb CMD_MAX
status_msg:	resb 80
buflen:		resd 1
cursor:		resd 1
row0:		resd 1
pref_col:	resd 1
cmdlen:		resd 1
dirty:		resd 1
mode:		resb 1
onebyte:	resb 1
