# Renk Tanımlamaları
GREY		=	\033[030m
RED		=	\033[031m
GREEN		=	\033[032m
YELLOW		=	\033[033m
BLUE		=	\033[034m
MAGENTA		=	\033[035m
CYAN		=	\033[036m
BOLD		=	\033[1m
RESET		=	\033[0m

# **************************************************************************** #
# 💾 VARIABLES
# **************************************************************************** #

KERNEL_BIN		=	build/kernel.bin
KERNEL_OUT		=	./target/x86_64-kernel/release/libkernel.a
KERNEL_DEBUG_OUT	=	./target/x86_64-kernel/debug/libkernel.a

ISO_OUT			=	build/kernel.iso
ISO_FULL_OUT		=	build/kernel-full.iso

BOOT			=	./multiboot/header.asm
# PR1 bring-up: only the Multiboot2 + long-mode trampoline is linked.
# exceptions/timer/syscall asm return in later PRs.
LINKER			=	linker/linker.ld
ASM_OBJS		=	build/boot.o build/exceptions.o build/irq.o build/syscall.o
IRQ_ASM			=	./multiboot/irq.asm
EXCEPTIONS_ASM		=	./multiboot/exceptions.asm
SYSCALL_ASM		=	./multiboot/syscall.asm
NASM_FMT		=	elf64
LD_EMUL			=	elf_x86_64

# GDB remote port used by QEMU (-s is an alias for tcp::1234)
GDB_PORT		=	1234
GDB_SCRIPT		=	gdb/kfs.gdb
# -nx: ignore ~/.gdbinit and local auto-load (.gdbinit safe-path warnings)
# -x:  load our project script explicitly
GDB_FLAGS		=	-q -nx -x $(GDB_SCRIPT)
QEMU_DEBUG_FLAGS	=	-s -S -no-reboot -no-shutdown
QEMU_COMMON_FLAGS	=
# **************************************************************************** #
# 🔧 TOOL DETECTION (soft — hard errors only when a target needs them)
# **************************************************************************** #

GRUB_MKRESCUE	=	$(shell which grub2-mkrescue 2>/dev/null || which grub-mkrescue 2>/dev/null)
GRUB_MODULE_DIR	=	$(shell [ -d /usr/lib/grub/i386-pc ] && echo /usr/lib/grub/i386-pc || ([ -d /usr/lib64/grub/i386-pc ] && echo /usr/lib64/grub/i386-pc))
QEMU_SYSTEM	=	$(shell which qemu-system-x86_64 2>/dev/null || which qemu 2>/dev/null)
LD		=	$(shell which ld 2>/dev/null || which ld.bfd 2>/dev/null)
NASM		=	$(shell which nasm 2>/dev/null)
CARGO		=	$(shell which cargo 2>/dev/null)
RUSTC		=	$(shell which rustc 2>/dev/null)
GDB		=	$(shell which gdb-multiarch 2>/dev/null || which gdb 2>/dev/null)

define require_tool
	@if [ -z "$(1)" ]; then echo -e "$(BOLD)$(RED)[✗] $(2) not found.$(RESET)"; exit 1; fi
endef

# **************************************************************************** #
# 📖 RULES
# **************************************************************************** #

# Default: build ISO and boot in QEMU (`make` with no target)
.DEFAULT_GOAL := all

SRCS = $(shell find src -name "*.rs" 2>/dev/null)

all: run

# --------------------------------------------------------------------------- #
# Help
# --------------------------------------------------------------------------- #

help:
	@echo -e "$(BOLD)$(CYAN)KFS — Kernel From Scratch$(RESET)"
	@echo ""
	@echo -e "$(BOLD)Usage:$(RESET)  make $(YELLOW)<target>$(RESET)"
	@echo ""
	@echo -e "$(BOLD)$(GREEN)Build$(RESET)"
	@echo -e "  $(YELLOW)build$(RESET)            Build release kernel → $(KERNEL_BIN)"
	@echo -e "  $(YELLOW)build_debug$(RESET)      Build debug kernel (symbols, no LTO) → $(KERNEL_BIN)"
	@echo -e "  $(YELLOW)iso$(RESET)              Build Multiboot ISO → $(ISO_OUT)"
	@echo -e "  $(YELLOW)iso-full$(RESET)         Build ISO with fuller GRUB modules → $(ISO_FULL_OUT)"
	@echo ""
	@echo -e "$(BOLD)$(GREEN)Run$(RESET)"
	@echo -e "  $(YELLOW)run$(RESET)              Build release + boot with QEMU (-kernel)"
	@echo -e "  $(YELLOW)run-iso$(RESET)          Build ISO + boot from CD in QEMU"
	@echo -e "  $(YELLOW)run-iso-full$(RESET)     Boot the full ISO in QEMU"
	@echo -e "  $(YELLOW)run-iso-term$(RESET)     Boot ISO with QEMU -nographic (no VGA UI)"
	@echo ""
	@echo -e "$(BOLD)$(GREEN)Debug (QEMU + GDB, step-by-step)$(RESET)"
	@echo -e "  $(YELLOW)debug$(RESET)            Build debug kernel, start QEMU frozen, attach GDB"
	@echo -e "  $(YELLOW)debug-qemu$(RESET)       Only start QEMU (wait for GDB on port $(GDB_PORT))"
	@echo -e "  $(YELLOW)debug-gdb$(RESET)        Only attach GDB to an already-running debug QEMU"
	@echo -e "  $(YELLOW)debug-iso$(RESET)        Same as debug, but boot the Multiboot ISO path"
	@echo ""
	@echo -e "  $(BOLD)Typical session:$(RESET)"
	@echo -e "    $$ make debug"
	@echo -e "    (gdb) break kmain          # or: break _start"
	@echo -e "    (gdb) continue             # run until breakpoint"
	@echo -e "    (gdb) step / next / stepi  # step source / next / one instruction"
	@echo -e "    (gdb) info registers"
	@echo -e "    (gdb) quit                 # also stops QEMU"
	@echo ""
	@echo -e "  $(BOLD)Two-terminal session:$(RESET)"
	@echo -e "    terminal A:  make debug-qemu"
	@echo -e "    terminal B:  make debug-gdb"
	@echo ""
	@echo -e "$(BOLD)$(GREEN)Inspect$(RESET)"
	@echo -e "  $(YELLOW)size$(RESET)             Show kernel.bin / ISO sizes (subject ~10 MiB soft limit)"
	@echo -e "  $(YELLOW)userland$(RESET)         Build freestanding ELF apps → build/rootfs/bin/"
	@echo -e "  $(YELLOW)disk$(RESET)             ext2 disk image (includes /bin/hello)"
	@echo ""
	@echo -e "$(BOLD)$(GREEN)Cleanup$(RESET)"
	@echo -e "  $(YELLOW)clean$(RESET)            Remove build/"
	@echo -e "  $(YELLOW)fclean$(RESET)           clean + cargo clean"
	@echo -e "  $(YELLOW)re$(RESET)               clean then all"
	@echo ""
	@echo -e "$(BOLD)$(GREEN)Help$(RESET)"
	@echo -e "  $(YELLOW)help$(RESET)             Show this message"
	@echo -e "  $(YELLOW)all$(RESET) / $(YELLOW)make$(RESET)       Default: same as $(YELLOW)run-iso$(RESET)"
	@echo ""

# --------------------------------------------------------------------------- #
# Build
# --------------------------------------------------------------------------- #

build: userland ${SRCS}
	$(call require_tool,$(NASM),nasm)
	$(call require_tool,$(CARGO),cargo)
	$(call require_tool,$(LD),ld)
	@mkdir -p build
	@${NASM} -f ${NASM_FMT} ${BOOT} -o build/boot.o
	@${NASM} -f ${NASM_FMT} ${EXCEPTIONS_ASM} -o build/exceptions.o
	@${NASM} -f ${NASM_FMT} ${IRQ_ASM} -o build/irq.o
	@${NASM} -f ${NASM_FMT} ${SYSCALL_ASM} -o build/syscall.o
	@${CARGO} build --no-default-features --release
	@echo -e "$(BOLD)$(GREEN)[✓] KERNEL BUILD DONE$(RESET)"
	@${LD} -m ${LD_EMUL} -T ${LINKER} -o ${KERNEL_BIN} ${ASM_OBJS} ${KERNEL_OUT}
	@echo -e "$(BOLD)$(GREEN)[✓] KERNEL LINK DONE → ${KERNEL_BIN}$(RESET)"

build_debug: userland ${SRCS}
	$(call require_tool,$(NASM),nasm)
	$(call require_tool,$(CARGO),cargo)
	$(call require_tool,$(LD),ld)
	@echo -e "$(BOLD)$(YELLOW)[…] KERNEL DEBUG BUILD$(RESET)"
	@mkdir -p build
	@${NASM} -f ${NASM_FMT} -g -F dwarf ${BOOT} -o build/boot.o
	@${NASM} -f ${NASM_FMT} -g -F dwarf ${EXCEPTIONS_ASM} -o build/exceptions.o
	@${NASM} -f ${NASM_FMT} -g -F dwarf ${IRQ_ASM} -o build/irq.o
	@${NASM} -f ${NASM_FMT} -g -F dwarf ${SYSCALL_ASM} -o build/syscall.o
	@${CARGO} build
	@echo -e "$(BOLD)$(GREEN)[✓] KERNEL BUILD DONE$(RESET)"
	@${LD} -m ${LD_EMUL} -T ${LINKER} -o ${KERNEL_BIN} ${ASM_OBJS} ${KERNEL_DEBUG_OUT}
	@echo -e "$(BOLD)$(GREEN)[✓] KERNEL LINK DONE → ${KERNEL_BIN}$(RESET)"

# Soft size report (subject often mentions ~10 MiB; BSS + image should stay lean)
size: build
	@echo -e "$(BOLD)$(CYAN)Kernel / artifact sizes$(RESET)"
	@ls -lh ${KERNEL_BIN} 2>/dev/null || true
	@if command -v size >/dev/null 2>&1; then size ${KERNEL_BIN}; fi
	@if [ -f ${ISO_OUT} ]; then ls -lh ${ISO_OUT}; fi
	@if [ -f ${DISK_IMG} ]; then ls -lh ${DISK_IMG}; fi
	@echo -e "$(BOLD)Scrollback BSS estimate:$(RESET) 6×48×80×25×2 = $$((6*48*80*25*2/1024)) KiB"

# --------------------------------------------------------------------------- #
# Run
# --------------------------------------------------------------------------- #

# ext2 disk image for IDE (-hda)
DISK_IMG		=	build/disk.img
USERLAND_HELLO	=	build/rootfs/bin/hello
USERLAND_SRC	=	userland/hello.asm
USERLAND_LD	=	userland/user.ld

EMBEDDED_HELLO_RS	=	src/embedded_hello.rs

# Freestanding x86_64 ET_EXEC + embed into Rust for `run` / `hello`
userland: ${USERLAND_SRC} ${USERLAND_LD}
	$(call require_tool,$(NASM),nasm)
	$(call require_tool,$(LD),ld)
	@mkdir -p build/userland build/rootfs/bin
	@${NASM} -f ${NASM_FMT} ${USERLAND_SRC} -o build/userland/hello.o
	@${LD} -m ${LD_EMUL} -T ${USERLAND_LD} -o ${USERLAND_HELLO} build/userland/hello.o
	@python3 -c "d=open('${USERLAND_HELLO}','rb').read(); open('${EMBEDDED_HELLO_RS}','w').write('//! Auto-generated by make userland — do not edit.\n#[allow(dead_code)]\npub static HELLO_ELF: &[u8] = &['+','.join(str(b) for b in d)+'];\n')"
	@echo -e "$(BOLD)$(GREEN)[✓] USERLAND ${USERLAND_HELLO} → ${EMBEDDED_HELLO_RS}$(RESET)"

disk: userland
	@mkdir -p build/rootfs/docs build/rootfs/bin
	@echo 'Hello from munux ext2!' > build/rootfs/hello.txt
	@echo 'second line' >> build/rootfs/hello.txt
	@echo 'readme content' > build/rootfs/docs/readme.txt
	@rm -f ${DISK_IMG}
	@dd if=/dev/zero of=${DISK_IMG} bs=1M count=32 status=none
	@mkfs.ext2 -F -q -b 1024 -d build/rootfs ${DISK_IMG}
	@echo -e "$(BOLD)$(GREEN)[✓] DISK IMAGE ${DISK_IMG}$(RESET)"

# PR1: Multiboot2 ELF64 needs GRUB (QEMU -kernel does not load MB2 ELF64).
run: run-iso

run-kernel-note:
	@echo -e "$(YELLOW)Note: use 'make run' / 'make run-iso' (Multiboot2 via GRUB).$(RESET)"
	@echo -e "$(YELLOW)QEMU -kernel cannot load this Multiboot2 x86_64 ELF.$(RESET)"

iso: build
	$(call require_tool,$(GRUB_MKRESCUE),grub-mkrescue)
	@if [ -z "$(GRUB_MODULE_DIR)" ]; then \
		echo -e "$(BOLD)$(RED)[✗] GRUB i386-pc modules not found (install grub-pc-bin).$(RESET)"; \
		exit 1; \
	fi
	@mkdir -p build/iso/boot/grub
	@cp grub/grub.cfg build/iso/boot/grub
	@cp ${KERNEL_BIN} build/iso/boot
	@${GRUB_MKRESCUE} -o ${ISO_OUT} build/iso --directory=${GRUB_MODULE_DIR} \
		--modules="multiboot2" --locales="" --fonts="" --themes=""
	@echo -e "$(BOLD)$(GREEN)[✓] KERNEL ISO BUILD → ${ISO_OUT}$(RESET)"

iso-full: build
	$(call require_tool,$(GRUB_MKRESCUE),grub-mkrescue)
	@if [ -z "$(GRUB_MODULE_DIR)" ]; then \
		echo -e "$(BOLD)$(RED)[✗] GRUB i386-pc modules not found (install grub-pc-bin).$(RESET)"; \
		exit 1; \
	fi
	@mkdir -p build/iso/boot/grub
	@cp grub/grub.cfg build/iso/boot/grub
	@cp ${KERNEL_BIN} build/iso/boot
	@${GRUB_MKRESCUE} -o ${ISO_FULL_OUT} build/iso --directory=${GRUB_MODULE_DIR} --modules="multiboot2"
	@echo -e "$(BOLD)$(GREEN)[✓] KERNEL FULL ISO BUILD → ${ISO_FULL_OUT}$(RESET)"

# IDE layout for our ATA driver (primary master = index 0):
#   index 0 = ext2 disk (build/disk.img)
#   index 1 = GRUB ISO (cdrom)
# Do not put two drives on index 0 — QEMU errors: "drive with bus=0, unit=0 exists"
# Primary master = ext2 disk; IDE index 1 = GRUB ISO (cdrom)
run-iso: iso disk
	$(call require_tool,$(QEMU_SYSTEM),qemu-system-x86_64)
	@${QEMU_SYSTEM} -m 512M \
		-drive format=raw,file=${DISK_IMG},if=ide,index=0,media=disk \
		-drive format=raw,file=${ISO_OUT},if=ide,index=1,media=cdrom \
		-boot order=d \
		-monitor stdio
	@echo -e "\n$(BOLD)$(CYAN)[✓] KERNEL EXIT DONE$(RESET)"

run-iso-full: iso-full disk
	$(call require_tool,$(QEMU_SYSTEM),qemu-system-x86_64)
	@${QEMU_SYSTEM} -m 4G \
		-drive format=raw,file=${DISK_IMG},if=ide,index=0,media=disk \
		-drive format=raw,file=${ISO_FULL_OUT},if=ide,index=1,media=cdrom \
		-boot order=d
	@echo -e "\n$(BOLD)$(CYAN)[✓] KERNEL EXIT DONE$(RESET)"

run-iso-term: iso disk
	$(call require_tool,$(QEMU_SYSTEM),qemu-system-x86_64)
	@${QEMU_SYSTEM} -m 4G \
		-drive format=raw,file=${DISK_IMG},if=ide,index=0,media=disk \
		-drive format=raw,file=${ISO_OUT},if=ide,index=1,media=cdrom \
		-boot order=d -nographic
	@echo -e "\n$(BOLD)$(CYAN)[✓] KERNEL EXIT DONE$(RESET)"

# --------------------------------------------------------------------------- #
# Debug — QEMU GDB stub + GDB client (step-by-step)
# --------------------------------------------------------------------------- #
# QEMU:  -s  → GDB server on tcp::$(GDB_PORT)
#        -S  → do not start CPU until GDB connects and continues
#
# Flow (make debug):
#   1. build_debug
#   2. start QEMU in background, frozen at reset
#   3. launch GDB with gdb/kfs.gdb (loads symbols, connects, breaks on kmain)

debug: build_debug
	$(call require_tool,$(QEMU_SYSTEM),qemu-system-x86_64)
	$(call require_tool,$(GDB),gdb)
	@echo -e "$(BOLD)$(YELLOW)[…] Starting QEMU (GDB stub :$(GDB_PORT), CPU frozen)$(RESET)"
	@echo -e "$(BOLD)$(YELLOW)[…] Attaching GDB — use: continue / step / next / stepi$(RESET)"
	@${QEMU_SYSTEM} -kernel ${KERNEL_BIN} $(QEMU_DEBUG_FLAGS) -serial stdio & \
		echo $$! > build/qemu-debug.pid; \
		sleep 0.4; \
		${GDB} $(GDB_FLAGS); \
		if [ -f build/qemu-debug.pid ]; then \
			kill $$(cat build/qemu-debug.pid) 2>/dev/null || true; \
			rm -f build/qemu-debug.pid; \
		fi
	@echo -e "\n$(BOLD)$(CYAN)[✓] KERNEL DEBUG EXIT DONE$(RESET)"

# Boot via ISO instead of -kernel (closer to real Multiboot/GRUB path).
# Does NOT depend on `iso` (that target rebuilds a release kernel).
debug-iso: build_debug disk
	$(call require_tool,$(QEMU_SYSTEM),qemu-system-x86_64)
	$(call require_tool,$(GDB),gdb)
	$(call require_tool,$(GRUB_MKRESCUE),grub-mkrescue)
	@if [ -z "$(GRUB_MODULE_DIR)" ]; then \
		echo -e "$(BOLD)$(RED)[✗] GRUB i386-pc modules not found (install grub-pc-bin).$(RESET)"; \
		exit 1; \
	fi
	@mkdir -p build/iso/boot/grub
	@cp grub/grub.cfg build/iso/boot/grub
	@cp ${KERNEL_BIN} build/iso/boot
	@${GRUB_MKRESCUE} -o ${ISO_OUT} build/iso --directory=${GRUB_MODULE_DIR} \
		--modules="multiboot" --locales="" --fonts="" --themes=""
	@echo -e "$(BOLD)$(YELLOW)[…] Starting QEMU ISO (GDB stub :$(GDB_PORT), CPU frozen)$(RESET)"
	@${QEMU_SYSTEM} -m 512M \
		-drive format=raw,file=${DISK_IMG},if=ide,index=0,media=disk \
		-drive format=raw,file=${ISO_OUT},if=ide,index=1,media=cdrom \
		-boot order=d \
		$(QEMU_DEBUG_FLAGS) & \
		echo $$! > build/qemu-debug.pid; \
		sleep 0.4; \
		${GDB} $(GDB_FLAGS); \
		if [ -f build/qemu-debug.pid ]; then \
			kill $$(cat build/qemu-debug.pid) 2>/dev/null || true; \
			rm -f build/qemu-debug.pid; \
		fi
	@echo -e "\n$(BOLD)$(CYAN)[✓] KERNEL DEBUG EXIT DONE$(RESET)"

# Terminal A: only QEMU (wait for GDB)
debug-qemu: build_debug
	$(call require_tool,$(QEMU_SYSTEM),qemu-system-x86_64)
	@echo -e "$(BOLD)$(YELLOW)[…] QEMU waiting for GDB on :$(GDB_PORT)$(RESET)"
	@echo -e "$(BOLD)$(CYAN)    In another terminal: make debug-gdb$(RESET)"
	@${QEMU_SYSTEM} -kernel ${KERNEL_BIN} $(QEMU_DEBUG_FLAGS) -serial stdio
	@echo -e "\n$(BOLD)$(CYAN)[✓] QEMU EXIT DONE$(RESET)"

# Terminal B: only GDB (connect to running QEMU)
debug-gdb:
	$(call require_tool,$(GDB),gdb)
	@if [ ! -f ${KERNEL_BIN} ]; then \
		echo -e "$(BOLD)$(RED)[✗] ${KERNEL_BIN} missing — run: make build_debug$(RESET)"; \
		exit 1; \
	fi
	@echo -e "$(BOLD)$(YELLOW)[…] Connecting GDB to localhost:$(GDB_PORT)$(RESET)"
	@${GDB} $(GDB_FLAGS)

# --------------------------------------------------------------------------- #
# Cleanup
# --------------------------------------------------------------------------- #

clean:
	@rm -rf build/
	@echo -e "$(BOLD)$(RED)[♻︎] DELETE build/ DONE$(RESET)"

fclean: clean
	$(call require_tool,$(CARGO),cargo)
	@${CARGO} clean
	@echo -e "$(BOLD)$(RED)[♻︎] CARGO CLEAN DONE$(RESET)"

re: clean all

.PHONY: all help \
	build build_debug size disk userland \
	run iso iso-full run-iso run-iso-full run-iso-term \
	debug debug-iso debug-qemu debug-gdb \
	clean fclean re
