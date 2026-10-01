// KizunaOS framebuffer + software backbuffer for QEMU ramfb.
// v0.1.0 desktop-alpha: render off-screen, present once, then allow live console updates.
#![allow(unsafe_op_in_unsafe_fn)]

use core::ptr::{addr_of_mut, copy, copy_nonoverlapping, read_volatile, write_volatile};

const FW_CFG_BASE: usize = 0x0902_0000;
const FW_CFG_DMA: usize  = FW_CFG_BASE + 0x10;
const FW_CFG_FILE_DIR: u16 = 0x0019;

const CTL_ERROR:  u32 = 0x01;
const CTL_READ:   u32 = 0x02;
const CTL_SELECT: u32 = 0x08;
const CTL_WRITE:  u32 = 0x10;

const FOURCC_XRGB8888: u32 = 0x3432_5258; // 'XR24'

pub const FB_W: usize = 800;
pub const FB_H: usize = 600;
const PIXELS: usize = FB_W * FB_H;

static mut FRONTBUFFER: [u32; PIXELS] = [0; PIXELS];
static mut BACKBUFFER: [u32; PIXELS] = [0; PIXELS];
static mut READY: bool = false;

#[repr(C)]
struct FwCfgDmaAccess {
    control: u32,
    length: u32,
    address: u64,
}

#[repr(C)]
struct RamfbCfg {
    addr: u64,
    fourcc: u32,
    flags: u32,
    width: u32,
    height: u32,
    stride: u32,
}

#[repr(C)]
struct FwCfgFile {
    size: u32,
    select: u16,
    reserved: u16,
    name: [u8; 56],
}

#[inline(always)]
unsafe fn front_ptr() -> *mut u32 {
    addr_of_mut!(FRONTBUFFER) as *mut u32
}

#[inline(always)]
unsafe fn back_ptr() -> *mut u32 {
    addr_of_mut!(BACKBUFFER) as *mut u32
}

unsafe fn phex(label: &str, val: u64) {
    crate::uart_write(label);
    let mut buf = [0u8; 18];
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..16 {
        let nib = ((val >> ((15 - i) * 4)) & 0xf) as u8;
        buf[2 + i] = if nib < 10 {
            b'0' + nib
        } else {
            b'a' + (nib - 10)
        };
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        crate::uart_write(s);
    }
    crate::uart_write("\n");
}

unsafe fn dma(control: u32, length: u32, address: u64) -> u32 {
    let access = FwCfgDmaAccess {
        control: control.to_be(),
        length: length.to_be(),
        address: address.to_be(),
    };
    let pa = &access as *const _ as u64;

    core::arch::asm!("dsb sy");
    write_volatile(FW_CFG_DMA as *mut u64, pa.to_be());
    core::arch::asm!("dsb sy");

    u32::from_be(read_volatile(&access.control))
}

unsafe fn find_file(name: &[u8]) -> Option<u16> {
    let mut count_be: u32 = 0;
    dma(
        (FW_CFG_FILE_DIR as u32) << 16 | CTL_SELECT | CTL_READ,
        4,
        &mut count_be as *mut u32 as u64,
    );

    let count = u32::from_be(count_be);
    crate::uart_write("ramfb: directory ready\n");

    for _ in 0..count {
        let mut entry = FwCfgFile {
            size: 0,
            select: 0,
            reserved: 0,
            name: [0u8; 56],
        };

        dma(
            CTL_READ,
            core::mem::size_of::<FwCfgFile>() as u32,
            &mut entry as *mut _ as u64,
        );

        if name.len() >= entry.name.len() {
            continue;
        }

        let mut hit = true;
        for (i, &b) in name.iter().enumerate() {
            if entry.name[i] != b {
                hit = false;
                break;
            }
        }

        if hit && entry.name[name.len()] == 0 {
            return Some(u16::from_be(entry.select));
        }
    }

    None
}

pub unsafe fn init() -> bool {
    let sel = match find_file(b"etc/ramfb") {
        Some(s) => {
            crate::uart_write("ramfb: found\n");
            s
        }
        None => {
            crate::uart_write("ramfb: NOT found\n");
            return false;
        }
    };

    let fb_addr = front_ptr() as u64;
    phex("ramfb: front=", fb_addr);

    let cfg = RamfbCfg {
        addr: fb_addr.to_be(),
        fourcc: FOURCC_XRGB8888.to_be(),
        flags: 0,
        width: (FB_W as u32).to_be(),
        height: (FB_H as u32).to_be(),
        stride: ((FB_W * 4) as u32).to_be(),
    };

    dma((sel as u32) << 16 | CTL_SELECT, 0, 0);
    let ctl = dma(CTL_WRITE, 28, &cfg as *const _ as u64);

    if ctl & CTL_ERROR != 0 {
        phex("ramfb: DMA error ctl=", ctl as u64);
        return false;
    }

    READY = true;
    crate::uart_write("ramfb: 800x600 XRGB8888 online\n");
    true
}

#[inline(always)]
pub fn is_ready() -> bool {
    unsafe { READY }
}

#[inline(always)]
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[inline(always)]
fn clipped_index(x: i32, y: i32) -> Option<usize> {
    if x < 0 || y < 0 || x >= FB_W as i32 || y >= FB_H as i32 {
        None
    } else {
        Some(y as usize * FB_W + x as usize)
    }
}

pub unsafe fn begin_frame(color: u32) {
    let back = back_ptr();
    for i in 0..PIXELS {
        write_volatile(back.add(i), color);
    }
}

pub unsafe fn present() {
    copy_nonoverlapping(back_ptr(), front_ptr(), PIXELS);
    core::arch::asm!("dsb sy");
}

#[inline(always)]
pub unsafe fn put_pixel(x: i32, y: i32, color: u32) {
    if let Some(i) = clipped_index(x, y) {
        write_volatile(back_ptr().add(i), color);
    }
}

#[inline(always)]
pub unsafe fn put_pixel_live(x: i32, y: i32, color: u32) {
    if let Some(i) = clipped_index(x, y) {
        write_volatile(back_ptr().add(i), color);
        write_volatile(front_ptr().add(i), color);
    }
}

#[inline(always)]
fn blend_channel(dst: u8, src: u8, alpha: u8) -> u8 {
    let a = alpha as u32;
    (((src as u32 * a) + (dst as u32 * (255 - a))) / 255) as u8
}

#[inline(always)]
fn blend_color(dst: u32, src: u32, alpha: u8) -> u32 {
    if alpha == 255 {
        return src;
    }
    if alpha == 0 {
        return dst;
    }

    let dr = ((dst >> 16) & 0xff) as u8;
    let dg = ((dst >> 8) & 0xff) as u8;
    let db = (dst & 0xff) as u8;

    let sr = ((src >> 16) & 0xff) as u8;
    let sg = ((src >> 8) & 0xff) as u8;
    let sb = (src & 0xff) as u8;

    rgb(
        blend_channel(dr, sr, alpha),
        blend_channel(dg, sg, alpha),
        blend_channel(db, sb, alpha),
    )
}

pub unsafe fn blend_pixel(x: i32, y: i32, color: u32, alpha: u8) {
    if let Some(i) = clipped_index(x, y) {
        let p = back_ptr().add(i);
        let out = blend_color(read_volatile(p), color, alpha);
        write_volatile(p, out);
    }
}

pub unsafe fn blend_pixel_live(x: i32, y: i32, color: u32, alpha: u8) {
    if let Some(i) = clipped_index(x, y) {
        let bp = back_ptr().add(i);
        let out = blend_color(read_volatile(bp), color, alpha);
        write_volatile(bp, out);
        write_volatile(front_ptr().add(i), out);
    }
}

pub unsafe fn fill_rect(x: i32, y: i32, w: i32, h: i32, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }

    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(FB_W as i32);
    let y1 = (y + h).min(FB_H as i32);

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let back = back_ptr();
    for yy in y0..y1 {
        let row = yy as usize * FB_W;
        for xx in x0..x1 {
            write_volatile(back.add(row + xx as usize), color);
        }
    }
}

pub unsafe fn fill_rect_live(x: i32, y: i32, w: i32, h: i32, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }

    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(FB_W as i32);
    let y1 = (y + h).min(FB_H as i32);

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let back = back_ptr();
    let front = front_ptr();
    for yy in y0..y1 {
        let row = yy as usize * FB_W;
        for xx in x0..x1 {
            let i = row + xx as usize;
            write_volatile(back.add(i), color);
            write_volatile(front.add(i), color);
        }
    }
}

pub unsafe fn stroke_rect(x: i32, y: i32, w: i32, h: i32, color: u32) {
    if w <= 1 || h <= 1 {
        return;
    }

    fill_rect(x, y, w, 1, color);
    fill_rect(x, y + h - 1, w, 1, color);
    fill_rect(x, y, 1, h, color);
    fill_rect(x + w - 1, y, 1, h, color);
}

pub unsafe fn fill_circle(cx: i32, cy: i32, radius: i32, color: u32) {
    if radius <= 0 {
        return;
    }

    let rr = radius * radius;
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= rr {
                put_pixel(cx + x, cy + y, color);
            }
        }
    }
}

/// Scroll a live rectangular region upward by dy pixels.
/// The backbuffer remains authoritative and the visible ramfb is kept in sync.
pub unsafe fn scroll_rect_up_live(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    dy: i32,
    clear_color: u32,
) {
    if w <= 0 || h <= 0 || dy <= 0 || dy >= h {
        return;
    }

    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(FB_W as i32);
    let y1 = (y + h).min(FB_H as i32);

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let width = (x1 - x0) as usize;
    let rows = (y1 - y0 - dy) as usize;
    let back = back_ptr();
    let front = front_ptr();

    for row in 0..rows {
        let src_y = y0 as usize + row + dy as usize;
        let dst_y = y0 as usize + row;
        let src = back.add(src_y * FB_W + x0 as usize);
        let dst = back.add(dst_y * FB_W + x0 as usize);
        copy(src, dst, width);

        let front_dst = front.add(dst_y * FB_W + x0 as usize);
        copy_nonoverlapping(dst, front_dst, width);
    }

    fill_rect_live(x0, y1 - dy, x1 - x0, dy, clear_color);
    core::arch::asm!("dsb sy");
}
