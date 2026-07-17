ENTRY(_start)
OUTPUT_FORMAT(elf32-i386)

/* Single loadable image starting at 1 MiB (standard Multiboot ELF layout). */
SECTIONS {
	. = 1M;

	/* Multiboot header must be in the first 8 KiB of the OS image. */
	.multiboot ALIGN(4) : {
		KEEP(*(.multiboot))
	}

	.text ALIGN(4K) : {
		*(.text .text.*)
	}

	.rodata ALIGN(4K) : {
		*(.rodata .rodata.*)
	}

	.data ALIGN(4K) : {
		*(.data .data.*)
	}

	/* Kernel stack: grows down toward stack_bottom. */
	.stack ALIGN(16) (NOLOAD) : {
		stack_bottom = .;
		. += 64K;
		stack_top = .;
	}

	.bss ALIGN(4K) : {
		*(COMMON)
		*(.bss .bss.*)
	}

	/* Symbols useful for later memory management */
	kernel_end = .;
}
