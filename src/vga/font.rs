//! Load a software 8×16 bitmap font into the VGA text-mode character generator.
//!
//! Display stays classic 80×25 text at `0xB8000` (MCP / qemu-connect scrape
//! unchanged). Only the **glyph shapes** change — plane 2 at `0xA0000`.

use crate::x86::io::{inb, outb};

use super::font8x16_data::FONT8X16;

const SEQ_ADDR: u16 = 0x3C4;
const SEQ_DATA: u16 = 0x3C5;
const GC_ADDR: u16 = 0x3CE;
const GC_DATA: u16 = 0x3CF;
const CRTC_ADDR: u16 = 0x3D4;
const CRTC_DATA: u16 = 0x3D5;

#[inline]
unsafe fn seq_set(index: u8, value: u8) {
    outb(SEQ_ADDR, index);
    outb(SEQ_DATA, value);
}

#[inline]
unsafe fn gc_set(index: u8, value: u8) {
    outb(GC_ADDR, index);
    outb(GC_DATA, value);
}

#[inline]
unsafe fn seq_get(index: u8) -> u8 {
    outb(SEQ_ADDR, index);
    inb(SEQ_DATA)
}

#[inline]
unsafe fn gc_get(index: u8) -> u8 {
    outb(GC_ADDR, index);
    inb(GC_DATA)
}

/// Write the 8×16 bitmap font into VGA plane 2 (character map).
///
/// Safe to call once at boot while still in VGA text mode. Identity map must
/// cover `0xA0000` (our early identity map does).
///
/// Sequence follows the standard plane-2 font load (OSDev VGA Fonts).
pub fn load_vga_bitmap_font() {
    unsafe {
        // Save sequencer / GC regs we touch so restore is exact.
        let save_seq2 = seq_get(0x02);
        let save_seq4 = seq_get(0x04);
        let save_gc5 = gc_get(0x05);
        let save_gc6 = gc_get(0x06);

        // Map mask = plane 2 only
        seq_set(0x02, 0x04);
        // Memory mode: disable odd/even, enable extended mem (value 0x07)
        seq_set(0x04, 0x07);

        // Graphics mode: write mode 0, disable odd/even
        gc_set(0x05, 0x00);
        // Misc: map A0000–AFFFF for plane access
        gc_set(0x06, 0x04);

        // Each character slot is 32 bytes in plane 2; glyphs use 16 rows.
        let plane = 0xA0000 as *mut u8;
        for ch in 0..256usize {
            let glyph = &FONT8X16[ch];
            let base = plane.add(ch * 32);
            for row in 0..16usize {
                base.add(row).write_volatile(glyph[row]);
            }
            for row in 16..32usize {
                base.add(row).write_volatile(0);
            }
        }

        // Restore original plane mapping for text mode (char+attr at B8000).
        seq_set(0x02, save_seq2);
        seq_set(0x04, save_seq4);
        // If boot left odd defaults, force known-good text values.
        if save_seq2 == 0 {
            seq_set(0x02, 0x03);
        }
        if save_seq4 & 0x04 == 0 {
            // ensure we leave text-compatible memory mode
            seq_set(0x04, 0x03);
        }
        gc_set(0x05, if save_gc5 == 0 { 0x10 } else { save_gc5 });
        gc_set(0x06, if (save_gc6 & 0x0C) == 0x04 { 0x0E } else { save_gc6 });
        // Prefer classic text restore (most QEMU/VGA BIOS text modes):
        seq_set(0x02, 0x03);
        seq_set(0x04, 0x03);
        gc_set(0x05, 0x10);
        gc_set(0x06, 0x0E);

        // 16 scanlines per character cell (matches 8×16 font).
        outb(CRTC_ADDR, 0x09);
        let msl = inb(CRTC_DATA);
        outb(CRTC_DATA, (msl & 0xE0) | 0x0F);
    }
}
