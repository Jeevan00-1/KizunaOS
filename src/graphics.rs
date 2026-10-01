// KizunaOS text renderer using a pre-rasterized no_std Noto Sans Mono font.
#![allow(unsafe_op_in_unsafe_fn)]

use noto_sans_mono_bitmap::{
    get_raster,
    get_raster_width,
    FontWeight,
    RasterHeight,
};

use crate::framebuffer;

const FONT_WEIGHT: FontWeight = FontWeight::Regular;
const FONT_HEIGHT: RasterHeight = RasterHeight::Size20;

#[inline(always)]
pub fn char_width() -> i32 {
    get_raster_width(FONT_WEIGHT, FONT_HEIGHT) as i32
}

#[inline(always)]
pub const fn line_height() -> i32 {
    24
}

pub fn text_width(text: &str) -> i32 {
    text.chars().count() as i32 * char_width()
}

unsafe fn draw_char_inner(x: i32, y: i32, ch: char, color: u32, live: bool) {
    let raster = get_raster(ch, FONT_WEIGHT, FONT_HEIGHT)
        .or_else(|| get_raster('?', FONT_WEIGHT, FONT_HEIGHT));

    let Some(raster) = raster else {
        return;
    };

    for (row_i, row) in raster.raster().iter().enumerate() {
        for (col_i, &alpha) in row.iter().enumerate() {
            if alpha == 0 {
                continue;
            }

            let px = x + col_i as i32;
            let py = y + row_i as i32;

            if live {
                framebuffer::blend_pixel_live(px, py, color, alpha);
            } else {
                framebuffer::blend_pixel(px, py, color, alpha);
            }
        }
    }
}

pub unsafe fn draw_char(x: i32, y: i32, ch: char, color: u32) {
    draw_char_inner(x, y, ch, color, false);
}

pub unsafe fn draw_char_live(x: i32, y: i32, ch: char, color: u32) {
    draw_char_inner(x, y, ch, color, true);
}

pub unsafe fn draw_text(mut x: i32, y: i32, text: &str, color: u32) {
    let cw = char_width();
    for ch in text.chars() {
        draw_char(x, y, ch, color);
        x += cw;
    }
}

pub unsafe fn draw_text_live(mut x: i32, y: i32, text: &str, color: u32) {
    let cw = char_width();
    for ch in text.chars() {
        draw_char_live(x, y, ch, color);
        x += cw;
    }
}
