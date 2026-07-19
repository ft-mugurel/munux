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

/// Write the 8×16 bitmap font into VGA plane 2 (character map).
///
/// Safe to call once at boot while still in VGA text mode. Identity map must
/// cover `0xA0000` (our early identity map does).
pub fn load_vga_bitmap_font() {
    unsafe {
        // --- unlock plane 2 for sequential write (OSDev / VGA font load) ---
        // Sequencer: map mask = plane 2 only
        outb(SEQ_ADDR, 0x02);
        outb(SEQ_DATA, 0x04);
        // Sequencer memory mode: disable odd/even, enable extended memory
        outb(SEQ_ADDR, 0x04);
        let seq4 = inb(SEQ_DATA);
        outb(SEQ_DATA, (seq4 | 0x04) & !0x08); // set bit2 (ext), clear bit3 (O/E)

        // Graphics controller: write mode 0, disable odd/even
        outb(GC_ADDR, 0x05);
        let gc5 = inb(GC_DATA);
        outb(GC_DATA, gc5 & !0x10); // clear odd/even
        // Misc: map memory to A0000h
        outb(GC_ADDR, 0x06);
        let gc6 = inb(GC_DATA);
        outb(GC_DATA, (gc6 & !0x0C) | 0x04);

        // Each character slot is 32 bytes in plane 2; we use 16 rows of bitmap.
        let plane = 0xA0000 as *mut u8;
        for ch in 0..256usize {
            let glyph = &FONT8X16[ch];
            let base = plane.add(ch * 32);
            for row in 0..16usize {
                base.add(row).write_volatile(glyph[row]);
            }
            // Clear remainder of the 32-byte slot
            for row in 16..32usize {
                base.add(row).write_volatile(0);
            }
        }

        // --- restore text-mode plane config ---
        outb(SEQ_ADDR, 0x02);
        outb(SEQ_DATA, 0x03); // planes 0+1 (char+attr) for normal text
        outb(SEQ_ADDR, 0x04);
        outb(SEQ_DATA, 0x03); // odd/even on, chain-4-ish text defaults
        outb(GC_ADDR, 0x05);
        outb(GC_DATA, 0x10); // odd/even enable, write mode 0
        outb(GC_ADDR, 0x06);
        outb(GC_DATA, 0x0E); // B8000 text map, odd/even

        // 16 scanlines per character cell (matches 8×16 font).
        outb(CRTC_ADDR, 0x09);
        let msl = inb(CRTC_DATA);
        outb(CRTC_DATA, (msl & 0xE0) | 0x0F); // max scan line = 15
    }
}
