use super::cursor;

pub const VGA_BUFFER: *mut u16 = 0xB8000 as *mut u16;
pub const VGA_WIDTH: usize = 80;
pub const VGA_HEIGHT: usize = 25;
const VGA_SIZE: usize = VGA_WIDTH * VGA_HEIGHT;
const VIRTUAL_SCREENS: usize = 6;
/// Scrollback depth. Keep modest so BSS stays well under the 10 MB subject limit
/// (6 screens × lines × 80 × 25 × 2 bytes).
pub(super) const SCROLLBACK_LINES: usize = 48;

#[derive(Copy, Clone)]
pub(super) struct ScreenCursor {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(pub u8);

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

pub(super) static mut CURRENT_COLOR: ColorCode = ColorCode::new(Color::White, Color::Black);
pub(super) static mut SCREEN_BUFFERS: [[u16; VGA_SIZE * SCROLLBACK_LINES]; VIRTUAL_SCREENS] =
    [[0; VGA_SIZE * SCROLLBACK_LINES]; VIRTUAL_SCREENS];
pub(super) static mut SCREEN_CURSORS: [ScreenCursor; VIRTUAL_SCREENS] =
    [ScreenCursor { x: 0, y: 0 }; VIRTUAL_SCREENS];
pub(super) static mut SCREEN_CURSOR_VISIBLE: [bool; VIRTUAL_SCREENS] = [true; VIRTUAL_SCREENS];
pub(super) static mut SCREEN_USED_LINES: [usize; VIRTUAL_SCREENS] = [1; VIRTUAL_SCREENS];
pub(super) static mut SCREEN_VIEWPORTS: [usize; VIRTUAL_SCREENS] = [0; VIRTUAL_SCREENS];
static mut ACTIVE_SCREEN: usize = 0;

fn blank_cell() -> u16 {
    unsafe { (b' ' as u16) | ((CURRENT_COLOR.0 as u16) << 8) }
}

pub(super) fn cell_index(line: usize, column: usize) -> usize {
    line * VGA_WIDTH + column
}

pub(super) fn clear_buffer_line(screen_index: usize, line: usize) {
    let blank = blank_cell();
    unsafe {
        for column in 0..VGA_WIDTH {
            SCREEN_BUFFERS[screen_index][cell_index(line, column)] = blank;
        }
    }
}

pub(super) fn shift_buffer_up(screen_index: usize) {
    unsafe {
        for line in 1..SCROLLBACK_LINES {
            for column in 0..VGA_WIDTH {
                let src = cell_index(line, column);
                let dst = cell_index(line - 1, column);
                SCREEN_BUFFERS[screen_index][dst] = SCREEN_BUFFERS[screen_index][src];
            }
        }

        clear_buffer_line(screen_index, SCROLLBACK_LINES - 1);

        if SCREEN_CURSORS[screen_index].y > 0 {
            SCREEN_CURSORS[screen_index].y -= 1;
        }
    }
}

pub(super) fn visible_top_line(screen_index: usize) -> usize {
    unsafe {
        let used_lines = SCREEN_USED_LINES[screen_index].min(SCROLLBACK_LINES);
        let max_scroll = used_lines.saturating_sub(VGA_HEIGHT);
        let viewport = SCREEN_VIEWPORTS[screen_index].min(max_scroll);
        max_scroll.saturating_sub(viewport)
    }
}

pub(super) fn render_screen(screen_index: usize) {
    unsafe {
        let top_line = visible_top_line(screen_index);
        let used_lines = SCREEN_USED_LINES[screen_index].min(SCROLLBACK_LINES);

        for row in 0..VGA_HEIGHT {
            let source_line = top_line + row;
            for column in 0..VGA_WIDTH {
                let value = if source_line < used_lines {
                    SCREEN_BUFFERS[screen_index][cell_index(source_line, column)]
                } else {
                    blank_cell()
                };
                VGA_BUFFER
                    .offset(cell_index(row, column) as isize)
                    .write_volatile(value);
            }
        }
    }

    cursor::sync_hardware_cursor(screen_index);
}

// pub(super) fn set_cursor_visible(screen_index: usize, visible: bool) {
//     unsafe {
//         SCREEN_CURSOR_VISIBLE[screen_index] = visible;
//     }
//     render_screen(screen_index);
// }

pub(super) fn cursor_visible(screen_index: usize) -> bool {
    unsafe { SCREEN_CURSOR_VISIBLE[screen_index] }
}

pub(super) fn current_screen_index() -> usize {
    unsafe { ACTIVE_SCREEN }
}

pub(super) fn current_cursor() -> ScreenCursor {
    unsafe { SCREEN_CURSORS[ACTIVE_SCREEN] }
}

pub(super) fn sync_screen_state(screen_index: usize) {
    unsafe {
        SCREEN_USED_LINES[screen_index] = SCREEN_USED_LINES[screen_index].min(SCROLLBACK_LINES);
        SCREEN_VIEWPORTS[screen_index] = SCREEN_VIEWPORTS[screen_index]
            .min(SCREEN_USED_LINES[screen_index].saturating_sub(VGA_HEIGHT));
    }
}

pub(super) fn write_cell(screen_index: usize, line: usize, column: usize, value: u16) {
    unsafe {
        SCREEN_BUFFERS[screen_index][cell_index(line, column)] = value;
    }
}

#[allow(dead_code)]
pub fn change_color(color: ColorCode) {
    unsafe {
        CURRENT_COLOR.0 = color.0;
    }
}

#[allow(dead_code)]
pub fn clear() {
    let screen_index = current_screen_index();
    unsafe {
        for line in 0..SCROLLBACK_LINES {
            clear_buffer_line(screen_index, line);
        }
        SCREEN_CURSORS[screen_index] = ScreenCursor { x: 0, y: 0 };
        SCREEN_CURSOR_VISIBLE[screen_index] = true;
        SCREEN_USED_LINES[screen_index] = 1;
        SCREEN_VIEWPORTS[screen_index] = 0;
    }
    render_screen(screen_index);
}

pub fn print(str: &str) {
    super::print::write_str(str);
}

pub fn print_char(c: char) {
    super::print::write_char(c);
}

// #[allow(dead_code)]
pub fn newline() {
    super::print::newline();
}

pub fn move_cursor_left() {
    cursor::move_left();
}

pub fn move_cursor_right() {
    cursor::move_right();
}

pub fn move_cursor_up() {
    cursor::move_up();
}

pub fn move_cursor_down() {
    cursor::move_down();
}

pub fn scroll_view_up() {
    let screen_index = current_screen_index();
    unsafe {
        let max_scroll = SCREEN_USED_LINES[screen_index].saturating_sub(VGA_HEIGHT);
        if SCREEN_VIEWPORTS[screen_index] < max_scroll {
            SCREEN_VIEWPORTS[screen_index] += 1;
        }
    }
    render_screen(screen_index);
}

pub fn scroll_view_down() {
    let screen_index = current_screen_index();
    unsafe {
        if SCREEN_VIEWPORTS[screen_index] > 0 {
            SCREEN_VIEWPORTS[screen_index] -= 1;
        }
    }
    render_screen(screen_index);
}

pub fn init_virtual_screens() {
    unsafe {
        for screen_index in 0..VIRTUAL_SCREENS {
            for line in 0..SCROLLBACK_LINES {
                clear_buffer_line(screen_index, line);
            }
            SCREEN_CURSORS[screen_index] = ScreenCursor { x: 0, y: 0 };
            SCREEN_CURSOR_VISIBLE[screen_index] = true;
            SCREEN_USED_LINES[screen_index] = 1;
            SCREEN_VIEWPORTS[screen_index] = 0;
        }
        ACTIVE_SCREEN = 0;
    }

    render_screen(0);
}

#[allow(dead_code)]
pub fn active_screen() -> usize {
    unsafe { ACTIVE_SCREEN }
}

pub fn switch_screen(screen_index: usize) {
    if screen_index >= VIRTUAL_SCREENS {
        return;
    }

    unsafe {
        ACTIVE_SCREEN = screen_index;
    }

    render_screen(screen_index);
}
