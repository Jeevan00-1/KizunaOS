// KizunaOS desktop-alpha shell.
// Kernel-resident for now; applications move to EL0 after MMU + syscalls.
#![allow(unsafe_op_in_unsafe_fn)]

use crate::{framebuffer, graphics};

const BG_TOP: u32 = 0x0d1117;
const BG_BOTTOM: u32 = 0x080b10;
const PANEL: u32 = 0x111720;
const PANEL_2: u32 = 0x151c26;
const BORDER: u32 = 0x273241;
const TEXT: u32 = 0xe8edf4;
const MUTED: u32 = 0x8b98a8;
const ACCENT: u32 = 0xd6dde7;
const TERMINAL_BG: u32 = 0x090d12;
const TERMINAL_TEXT: u32 = 0xdce4ee;
const TERMINAL_MUTED: u32 = 0x7f8b99;
const GREEN: u32 = 0x5fd38d;
const RED: u32 = 0xff6b6b;
const YELLOW: u32 = 0xf6c85f;

const WIN_X: i32 = 66;
const WIN_Y: i32 = 72;
const WIN_W: i32 = 668;
const WIN_H: i32 = 446;
const TITLE_H: i32 = 42;

const TERM_X: i32 = 88;
const TERM_Y: i32 = 132;
const TERM_W: i32 = 624;
const TERM_H: i32 = 352;
const TERM_PAD_X: i32 = 14;
const TERM_PAD_Y: i32 = 14;

const CONSOLE_X0: i32 = TERM_X + TERM_PAD_X;
const CONSOLE_Y0: i32 = TERM_Y + TERM_PAD_Y;
const CONSOLE_RIGHT: i32 = TERM_X + TERM_W - TERM_PAD_X;
const CONSOLE_BOTTOM: i32 = TERM_Y + TERM_H - TERM_PAD_Y;

const POINTER_W: i32 = 13;
const POINTER_H: i32 = 19;

static mut READY: bool = false;
static mut CURSOR_X: i32 = CONSOLE_X0;
static mut CURSOR_Y: i32 = CONSOLE_Y0;

static mut POINTER_X: i32 = 400;
static mut POINTER_Y: i32 = 300;
static mut POINTER_DOWN: bool = false;
static mut POINTER_RIGHT: bool = false;
static mut POINTER_MIDDLE: bool = false;
static mut POINTER_VISIBLE: bool = false;
static mut TERMINAL_FOCUSED: bool = true;

#[inline(always)]
fn lerp_u8(a: u8, b: u8, t: u32, max: u32) -> u8 {
    let av = a as u32;
    let bv = b as u32;
    if bv >= av {
        (av + ((bv - av) * t / max)) as u8
    } else {
        (av - ((av - bv) * t / max)) as u8
    }
}

fn gradient_row(y: usize) -> u32 {
    let max = (framebuffer::FB_H - 1) as u32;
    let t = y as u32;

    let tr = ((BG_TOP >> 16) & 0xff) as u8;
    let tg = ((BG_TOP >> 8) & 0xff) as u8;
    let tb = (BG_TOP & 0xff) as u8;

    let br = ((BG_BOTTOM >> 16) & 0xff) as u8;
    let bg = ((BG_BOTTOM >> 8) & 0xff) as u8;
    let bb = (BG_BOTTOM & 0xff) as u8;

    framebuffer::rgb(
        lerp_u8(tr, br, t, max),
        lerp_u8(tg, bg, t, max),
        lerp_u8(tb, bb, t, max),
    )
}

unsafe fn draw_desktop_chrome() {
    framebuffer::begin_frame(BG_TOP);

    for y in 0..framebuffer::FB_H {
        framebuffer::fill_rect(0, y as i32, framebuffer::FB_W as i32, 1, gradient_row(y));
    }

    framebuffer::fill_rect(0, 0, framebuffer::FB_W as i32, 40, 0x0a0e13);
    framebuffer::fill_rect(0, 39, framebuffer::FB_W as i32, 1, BORDER);
    graphics::draw_text(18, 11, "KIZUNA OS", TEXT);
    graphics::draw_text(575, 11, "AARCH64  /  EL1", MUTED);

    framebuffer::fill_rect(WIN_X + 8, WIN_Y + 10, WIN_W, WIN_H, 0x05070a);
    framebuffer::fill_rect(WIN_X, WIN_Y, WIN_W, WIN_H, PANEL);
    framebuffer::stroke_rect(WIN_X, WIN_Y, WIN_W, WIN_H, BORDER);

    framebuffer::fill_rect(WIN_X + 1, WIN_Y + 1, WIN_W - 2, TITLE_H, PANEL_2);
    framebuffer::fill_rect(WIN_X + 1, WIN_Y + TITLE_H, WIN_W - 2, 1, BORDER);
    graphics::draw_text(WIN_X + 18, WIN_Y + 13, "Terminal", TEXT);

    framebuffer::fill_circle(WIN_X + WIN_W - 68, WIN_Y + 21, 5, RED);
    framebuffer::fill_circle(WIN_X + WIN_W - 48, WIN_Y + 21, 5, YELLOW);
    framebuffer::fill_circle(WIN_X + WIN_W - 28, WIN_Y + 21, 5, GREEN);

    framebuffer::fill_rect(TERM_X, TERM_Y, TERM_W, TERM_H, TERMINAL_BG);
    framebuffer::stroke_rect(TERM_X, TERM_Y, TERM_W, TERM_H, 0x1d2632);

    framebuffer::fill_rect(282, 548, 236, 36, 0x10161e);
    framebuffer::stroke_rect(282, 548, 236, 36, BORDER);
    framebuffer::fill_circle(307, 566, 10, ACCENT);
    graphics::draw_text(303, 558, "K", 0x11161d);
    graphics::draw_text(333, 557, "Terminal", TEXT);
    graphics::draw_text(430, 557, "0.1.0a", MUTED);

    framebuffer::fill_circle(549, 20, 4, GREEN);
}

unsafe fn draw_boot_card() {
    let x = CONSOLE_X0;
    let mut y = CONSOLE_Y0;

    graphics::draw_text(x, y, "KizunaOS 0.1.0 desktop alpha", TERMINAL_TEXT);
    y += graphics::line_height();
    graphics::draw_text(x, y, "ARM64 kernel / QEMU virt", TERMINAL_MUTED);
    y += graphics::line_height() * 2;

    graphics::draw_text(x, y, "boot", MUTED);
    graphics::draw_text(x + 80, y, "online", GREEN);
    y += graphics::line_height();

    graphics::draw_text(x, y, "exceptions", MUTED);
    graphics::draw_text(x + 110, y, "active", GREEN);
    y += graphics::line_height();

    graphics::draw_text(x, y, "heap", MUTED);
    graphics::draw_text(x + 80, y, "1 MiB allocator", TERMINAL_TEXT);
    y += graphics::line_height();

    graphics::draw_text(x, y, "display", MUTED);
    graphics::draw_text(x + 80, y, "ramfb / XRGB8888", TERMINAL_TEXT);
    y += graphics::line_height() * 2;

    CURSOR_X = CONSOLE_X0;
    CURSOR_Y = y;
}

const CURSOR_MASK: [u16; POINTER_H as usize] = [
    0b1000000000000,
    0b1100000000000,
    0b1110000000000,
    0b1111000000000,
    0b1111100000000,
    0b1111110000000,
    0b1111111000000,
    0b1111111100000,
    0b1111111110000,
    0b1111111111000,
    0b1111110000000,
    0b1110110000000,
    0b1100110000000,
    0b1000011000000,
    0b0000011000000,
    0b0000001100000,
    0b0000001100000,
    0,
    0,
];

unsafe fn restore_pointer_underlay() {
    if POINTER_VISIBLE {
        framebuffer::restore_front_from_back(
            POINTER_X - 1,
            POINTER_Y - 1,
            POINTER_W + 2,
            POINTER_H + 2,
        );
    }
}

unsafe fn draw_pointer_overlay() {
    if !POINTER_VISIBLE {
        return;
    }

    let fill = if POINTER_RIGHT {
        RED
    } else if POINTER_MIDDLE {
        YELLOW
    } else if POINTER_DOWN {
        GREEN
    } else {
        0xf4f7fb
    };
    let outline = 0x05070a;

    for row in 0..POINTER_H {
        let mask = CURSOR_MASK[row as usize];
        for col in 0..POINTER_W {
            let bit = 1u16 << (POINTER_W - 1 - col);
            if mask & bit == 0 {
                continue;
            }

            let edge = row == 0
                || col == 0
                || row == POINTER_H - 1
                || col == POINTER_W - 1
                || (row > 0 && CURSOR_MASK[(row - 1) as usize] & bit == 0)
                || (row + 1 < POINTER_H && CURSOR_MASK[(row + 1) as usize] & bit == 0)
                || (col > 0 && mask & (bit << 1) == 0)
                || (col + 1 < POINTER_W && mask & (bit >> 1) == 0);

            framebuffer::put_pixel_front(
                POINTER_X + col,
                POINTER_Y + row,
                if edge { outline } else { fill },
            );
        }
    }
}

#[inline(always)]
fn point_in(x: i32, y: i32, rx: i32, ry: i32, rw: i32, rh: i32) -> bool {
    x >= rx && y >= ry && x < rx + rw && y < ry + rh
}

unsafe fn draw_focus_ring_live() {
    framebuffer::stroke_rect_live(
        TERM_X,
        TERM_Y,
        TERM_W,
        TERM_H,
        if TERMINAL_FOCUSED { GREEN } else { 0x1d2632 },
    );
}

pub unsafe fn focus_terminal() {
    if !READY || TERMINAL_FOCUSED {
        return;
    }

    restore_pointer_underlay();
    TERMINAL_FOCUSED = true;
    draw_focus_ring_live();
    draw_pointer_overlay();
}

pub fn terminal_focused() -> bool {
    unsafe { TERMINAL_FOCUSED }
}

pub unsafe fn pointer_update(x: i32, y: i32, down: bool) {
    pointer_event(x, y, down, false, false);
}

pub unsafe fn pointer_event(x: i32, y: i32, left: bool, right: bool, middle: bool) {
    if !READY {
        return;
    }

    let next_x = x.clamp(0, framebuffer::FB_W as i32 - 1);
    let next_y = y.clamp(0, framebuffer::FB_H as i32 - 1);
    let left_pressed = left && !POINTER_DOWN;

    if POINTER_VISIBLE
        && next_x == POINTER_X
        && next_y == POINTER_Y
        && left == POINTER_DOWN
        && right == POINTER_RIGHT
        && middle == POINTER_MIDDLE
    {
        return;
    }

    restore_pointer_underlay();

    POINTER_X = next_x;
    POINTER_Y = next_y;
    POINTER_DOWN = left;
    POINTER_RIGHT = right;
    POINTER_MIDDLE = middle;
    POINTER_VISIBLE = true;

    if left_pressed {
        let focused = point_in(next_x, next_y, WIN_X, WIN_Y, WIN_W, WIN_H)
            || point_in(next_x, next_y, 282, 548, 236, 36);

        if focused != TERMINAL_FOCUSED {
            TERMINAL_FOCUSED = focused;
            draw_focus_ring_live();
        }
    }

    draw_pointer_overlay();
}

pub unsafe fn init() {
    draw_desktop_chrome();
    draw_boot_card();
    framebuffer::present();
    READY = true;
}

#[inline(always)]
pub fn is_ready() -> bool {
    unsafe { READY }
}

unsafe fn ensure_cursor_visible() {
    if CURSOR_Y + graphics::line_height() <= CONSOLE_BOTTOM {
        return;
    }

    framebuffer::scroll_rect_up_live(
        CONSOLE_X0,
        CONSOLE_Y0,
        CONSOLE_RIGHT - CONSOLE_X0,
        CONSOLE_BOTTOM - CONSOLE_Y0,
        graphics::line_height(),
        TERMINAL_BG,
    );

    CURSOR_Y -= graphics::line_height();
}

pub unsafe fn console_move_left() {
    if !READY {
        return;
    }

    let cw = graphics::char_width();
    if CURSOR_X - cw >= CONSOLE_X0 {
        CURSOR_X -= cw;
    }
}

pub unsafe fn console_move_right() {
    if !READY {
        return;
    }

    let cw = graphics::char_width();
    if CURSOR_X + cw < CONSOLE_RIGHT {
        CURSOR_X += cw;
    }
}

pub unsafe fn console_clear() {
    if !READY {
        return;
    }

    restore_pointer_underlay();

    framebuffer::fill_rect_live(
        CONSOLE_X0,
        CONSOLE_Y0,
        CONSOLE_RIGHT - CONSOLE_X0,
        CONSOLE_BOTTOM - CONSOLE_Y0,
        TERMINAL_BG,
    );

    CURSOR_X = CONSOLE_X0;
    CURSOR_Y = CONSOLE_Y0;

    draw_pointer_overlay();
}

pub unsafe fn console_putc(byte: u8) {
    if !READY {
        return;
    }

    restore_pointer_underlay();

    let cw = graphics::char_width();

    match byte {
        b'\r' => CURSOR_X = CONSOLE_X0,
        b'\n' => {
            CURSOR_X = CONSOLE_X0;
            CURSOR_Y += graphics::line_height();
            ensure_cursor_visible();
        }
        0x08 | 0x7f => {
            if CURSOR_X > CONSOLE_X0 {
                CURSOR_X -= cw;
                framebuffer::fill_rect_live(
                    CURSOR_X,
                    CURSOR_Y,
                    cw,
                    graphics::line_height(),
                    TERMINAL_BG,
                );
            }
        }
        b'\t' => {
            for _ in 0..4 {
                console_putc(b' ');
            }
        }
        0x20..=0x7e => {
            if CURSOR_X + cw > CONSOLE_RIGHT {
                CURSOR_X = CONSOLE_X0;
                CURSOR_Y += graphics::line_height();
                ensure_cursor_visible();
            }

            graphics::draw_char_live(CURSOR_X, CURSOR_Y, byte as char, TERMINAL_TEXT);
            CURSOR_X += cw;
        }
        _ => {}
    }

    draw_pointer_overlay();
}

pub unsafe fn panic_screen() {
    if !framebuffer::is_ready() {
        return;
    }

    POINTER_VISIBLE = false;

    framebuffer::begin_frame(0x12090b);
    framebuffer::fill_rect(0, 0, framebuffer::FB_W as i32, 6, RED);
    graphics::draw_text(54, 80, "KIZUNA OS", MUTED);
    graphics::draw_text(54, 132, "KERNEL PANIC", 0xffd7db);
    graphics::draw_text(54, 172, "Execution has been halted.", TEXT);
    graphics::draw_text(54, 203, "Check the serial console for diagnostics.", MUTED);
    framebuffer::present();
}
