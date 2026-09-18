// src/framebuffer.rs — ramfb framebuffer via fw_cfg DMA (QEMU virt)
use core::ptr::{read_volatile, write_volatile, addr_of_mut};

const FW_CFG_BASE: usize = 0x0902_0000;
const FW_CFG_DMA: usize  = FW_CFG_BASE + 0x10;

const FW_CFG_FILE_DIR: u16 = 0x0019;

const CTL_ERROR:  u32 = 0x01;
const CTL_READ:   u32 = 0x02;
const CTL_SELECT: u32 = 0x08;
const CTL_WRITE:  u32 = 0x10;

const FOURCC_XRGB8888: u32 = 0x3432_5258; // 'XR24'

pub const FB_W: u32 = 800;
pub const FB_H: u32 = 600;
static mut FRAMEBUFFER: [u32; (FB_W * FB_H) as usize] = [0; (FB_W * FB_H) as usize];

#[repr(C)]
struct FwCfgDmaAccess { control: u32, length: u32, address: u64 }

#[repr(C)]
struct RamfbCfg { addr: u64, fourcc: u32, flags: u32, width: u32, height: u32, stride: u32 }

#[repr(C)]
struct FwCfgFile { size: u32, select: u16, reserved: u16, name: [u8; 56] }

unsafe fn phex(label: &str, val: u64) {
    crate::uart_write(label);
    let mut buf = [0u8; 18];
    buf[0] = b'0'; buf[1] = b'x';
    for i in 0..16 {
        let nib = ((val >> ((15 - i) * 4)) & 0xf) as u8;
        buf[2 + i] = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
    }
    if let Ok(s) = core::str::from_utf8(&buf) { crate::uart_write(s); }
    crate::uart_write("\n");
}

unsafe fn dma(control: u32, length: u32, address: u64) -> u32 {
    let access = FwCfgDmaAccess {
        control: control.to_be(),
        length:  length.to_be(),
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
    dma((FW_CFG_FILE_DIR as u32) << 16 | CTL_SELECT | CTL_READ,
        4, &mut count_be as *mut u32 as u64);
    let count = u32::from_be(count_be);
    crate::uart_write("ramfb: dir count ok\n");

    for _ in 0..count {
        let mut e = FwCfgFile { size: 0, select: 0, reserved: 0, name: [0u8; 56] };
        dma(CTL_READ, core::mem::size_of::<FwCfgFile>() as u32,
            &mut e as *mut _ as u64);
        let mut hit = true;
        for (i, &b) in name.iter().enumerate() {
            if e.name[i] != b { hit = false; break; }
        }
        if hit && e.name[name.len()] == 0 {
            return Some(u16::from_be(e.select));
        }
    }
    None
}

pub unsafe fn init() -> bool {
    let sel = match find_file(b"etc/ramfb") {
        Some(s) => { crate::uart_write("ramfb: found\n"); s }
        None => { crate::uart_write("ramfb: NOT found\n"); return false }
    };
    let fb_addr = addr_of_mut!(FRAMEBUFFER) as u64;
    phex("ramfb: fb_addr=", fb_addr);
    phex("ramfb: sel=", sel as u64);

    let cfg = RamfbCfg {
        addr:   fb_addr.to_be(),
        fourcc: FOURCC_XRGB8888.to_be(),
        flags:  0,
        width:  FB_W.to_be(),
        height: FB_H.to_be(),
        stride: (FB_W * 4).to_be(),
    };
    dma((sel as u32) << 16 | CTL_SELECT, 0, 0);
    let ctl = dma(CTL_WRITE,
        28,
        &cfg as *const _ as u64);
    phex("ramfb: write ctl=", ctl as u64);
    if ctl & CTL_ERROR != 0 { crate::uart_write("ramfb: DMA ERROR\n"); return false; }
    true
}

pub unsafe fn clear(color: u32) {
    let base = addr_of_mut!(FRAMEBUFFER) as *mut u32;
    for i in 0..(FB_W * FB_H) as usize {
        write_volatile(base.add(i), color);
    }
}
