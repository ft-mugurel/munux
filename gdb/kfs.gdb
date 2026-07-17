# KFS kernel GDB helpers — used by `make debug` / `make debug-gdb`
# Loaded explicitly: gdb -nx -x gdb/kfs.gdb  (not auto-loaded as ./.gdbinit)
#
# QEMU must already be listening (started with -s -S), typically:
#   make debug          # one-shot
#   make debug-qemu     # terminal A, then debug-gdb in terminal B

set confirm off
set pagination off
set architecture i386
set disassembly-flavor intel

# Load ELF symbols (linked with debug staticlib + nasm -g)
file build/kernel.bin

# Connect to QEMU GDB stub (-s ⇒ tcp::1234)
target remote localhost:1234

# Stop early in kernel startup (change if you prefer _start only)
break kmain
# break _start

# Uncomment for more verbose traces:
# set debug remote 1

echo \n[KFS] Connected to QEMU. CPU is frozen.\n
echo [KFS] Useful commands:\n
echo   continue / c     - run until breakpoint or IRQ\n
echo   step / s         - step one source line (into calls)\n
echo   next / n         - step one source line (over calls)\n
echo   stepi / si       - step one instruction\n
echo   nexti / ni       - next instruction (over calls)\n
echo   break <sym>      - e.g. break load_gdt, break keyboard_interrupt_handler\n
echo   info registers   - dump GPRs / eflags\n
echo   x/16i $eip       - disassemble at PC\n
echo   quit / q         - leave GDB (make debug also stops QEMU)\n\n

# Do not auto-continue — you control the first step
