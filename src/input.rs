// KizunaOS VirtIO input stack for QEMU virt.
// Polling first: this gives us real keyboard/tablet input before the GIC/IRQ pass.
#![allow(unsafe_op_in_unsafe_fn)]

use core::{
    cell::UnsafeCell,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use virtio_drivers::{
    device::input::{AbsInfo, InputEvent, VirtIOInput},
    transport::{
        mmio::{MmioTransport, VirtIOHeader},
        DeviceType, Transport,
    },
    BufferDirection, Hal, PhysAddr, PAGE_SIZE,
};

const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const VIRTIO_MMIO_STRIDE: usize = 0x200;
const VIRTIO_MMIO_SLOTS: usize = 32;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

const KEY_ESC: u16 = 1;
const KEY_BACKSPACE: u16 = 14;
const KEY_TAB: u16 = 15;
const KEY_ENTER: u16 = 28;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_LEFTALT: u16 = 56;
const KEY_CAPSLOCK: u16 = 58;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_RIGHTALT: u16 = 100;
const KEY_HOME: u16 = 102;
const KEY_UP: u16 = 103;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;
const KEY_END: u16 = 107;
const KEY_DOWN: u16 = 108;
const KEY_DELETE: u16 = 111;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTMETA: u16 = 126;

const DMA_POOL_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[repr(align(4096))]
struct DmaPool {
    bytes: UnsafeCell<[u8; DMA_POOL_BYTES]>,
}

unsafe impl Sync for DmaPool {}

static DMA_POOL: DmaPool = DmaPool {
    bytes: UnsafeCell::new([0; DMA_POOL_BYTES]),
};
static DMA_NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct KizunaHal;

// The MMU is still disabled in this alpha, so kernel virtual addresses equal
// guest physical addresses. This early HAL is intentionally simple.
unsafe impl Hal for KizunaHal {
    fn dma_alloc(
        pages: usize,
        _direction: BufferDirection,
    ) -> (PhysAddr, NonNull<u8>) {
        let Some(bytes) = pages.checked_mul(PAGE_SIZE) else {
            return (0, NonNull::dangling());
        };

        let offset = DMA_NEXT.fetch_add(bytes, Ordering::SeqCst);
        if offset.checked_add(bytes).is_none() || offset + bytes > DMA_POOL_BYTES {
            return (0, NonNull::dangling());
        }

        let base = DMA_POOL.bytes.get() as *mut u8;
        let ptr = unsafe { base.add(offset) };
        unsafe { core::ptr::write_bytes(ptr, 0, bytes) };

        let vaddr = NonNull::new(ptr).unwrap();
        (ptr as usize as u64, vaddr)
    }

    unsafe fn dma_dealloc(
        _paddr: PhysAddr,
        _vaddr: NonNull<u8>,
        _pages: usize,
    ) -> i32 {
        // Input devices persist for the kernel lifetime, so this boot DMA arena
        // is monotonic until the real physical-page allocator arrives.
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as usize as *mut u8).unwrap()
    }

    unsafe fn share(
        buffer: NonNull<[u8]>,
        _direction: BufferDirection,
    ) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as u64
    }

    unsafe fn unshare(
        _paddr: PhysAddr,
        _buffer: NonNull<[u8]>,
        _direction: BufferDirection,
    ) {
    }
}

type InputDriver = VirtIOInput<KizunaHal, MmioTransport<'static>>;

struct InputState {
    pointer: Option<InputDriver>,
    keyboard: Option<InputDriver>,

    abs_x: AbsInfo,
    abs_y: AbsInfo,
    raw_x: u32,
    raw_y: u32,
    left_down: bool,
    right_down: bool,
    middle_down: bool,

    left_shift: bool,
    right_shift: bool,
    left_ctrl: bool,
    right_ctrl: bool,
    left_alt: bool,
    right_alt: bool,
    left_meta: bool,
    right_meta: bool,
    caps_lock: bool,
}

impl InputState {
    const fn new() -> Self {
        Self {
            pointer: None,
            keyboard: None,
            abs_x: AbsInfo {
                min: 0,
                max: 1,
                fuzz: 0,
                flat: 0,
                res: 0,
            },
            abs_y: AbsInfo {
                min: 0,
                max: 1,
                fuzz: 0,
                flat: 0,
                res: 0,
            },
            raw_x: 0,
            raw_y: 0,
            left_down: false,
            right_down: false,
            middle_down: false,
            left_shift: false,
            right_shift: false,
            left_ctrl: false,
            right_ctrl: false,
            left_alt: false,
            right_alt: false,
            left_meta: false,
            right_meta: false,
            caps_lock: false,
        }
    }
}

struct InputCell(UnsafeCell<InputState>);
unsafe impl Sync for InputCell {}

static INPUT: InputCell = InputCell(UnsafeCell::new(InputState::new()));

#[inline(always)]
unsafe fn state() -> &'static mut InputState {
    &mut *INPUT.0.get()
}

fn scale_axis(value: u32, info: &AbsInfo, extent: usize) -> i32 {
    if info.max <= info.min || extent <= 1 {
        return 0;
    }

    let clamped = value.clamp(info.min, info.max) - info.min;
    let span = info.max - info.min;
    ((clamped as u64 * (extent as u64 - 1)) / span as u64) as i32
}

fn key_to_ascii(code: u16, shift: bool, caps_lock: bool) -> Option<u8> {
    let letter = |lower: u8| {
        if shift ^ caps_lock {
            lower.to_ascii_uppercase()
        } else {
            lower
        }
    };

    Some(match code {
        1 => 0x1b,
        2 => if shift { b'!' } else { b'1' },
        3 => if shift { b'@' } else { b'2' },
        4 => if shift { b'#' } else { b'3' },
        5 => if shift { b'$' } else { b'4' },
        6 => if shift { b'%' } else { b'5' },
        7 => if shift { b'^' } else { b'6' },
        8 => if shift { b'&' } else { b'7' },
        9 => if shift { b'*' } else { b'8' },
        10 => if shift { b'(' } else { b'9' },
        11 => if shift { b')' } else { b'0' },
        12 => if shift { b'_' } else { b'-' },
        13 => if shift { b'+' } else { b'=' },
        14 => 0x08,
        15 => b'\t',
        16 => letter(b'q'),
        17 => letter(b'w'),
        18 => letter(b'e'),
        19 => letter(b'r'),
        20 => letter(b't'),
        21 => letter(b'y'),
        22 => letter(b'u'),
        23 => letter(b'i'),
        24 => letter(b'o'),
        25 => letter(b'p'),
        26 => if shift { b'{' } else { b'[' },
        27 => if shift { b'}' } else { b']' },
        28 => b'\n',
        30 => letter(b'a'),
        31 => letter(b's'),
        32 => letter(b'd'),
        33 => letter(b'f'),
        34 => letter(b'g'),
        35 => letter(b'h'),
        36 => letter(b'j'),
        37 => letter(b'k'),
        38 => letter(b'l'),
        39 => if shift { b':' } else { b';' },
        40 => if shift { b'"' } else { b'\'' },
        41 => if shift { b'~' } else { 0x60 },
        43 => if shift { b'|' } else { b'\\' },
        44 => letter(b'z'),
        45 => letter(b'x'),
        46 => letter(b'c'),
        47 => letter(b'v'),
        48 => letter(b'b'),
        49 => letter(b'n'),
        50 => letter(b'm'),
        51 => if shift { b'<' } else { b',' },
        52 => if shift { b'>' } else { b'.' },
        53 => if shift { b'?' } else { b'/' },
        57 => b' ',
        _ => return None,
    })
}

unsafe fn handle_pointer_event(s: &mut InputState, ev: InputEvent) -> bool {
    match (ev.event_type, ev.code) {
        (EV_ABS, ABS_X) => s.raw_x = ev.value,
        (EV_ABS, ABS_Y) => s.raw_y = ev.value,
        (EV_KEY, BTN_LEFT) => s.left_down = ev.value != 0,
        (EV_KEY, BTN_RIGHT) => s.right_down = ev.value != 0,
        (EV_KEY, BTN_MIDDLE) => s.middle_down = ev.value != 0,
        (EV_SYN, SYN_REPORT) => return true,
        _ => {}
    }
    false
}

unsafe fn handle_keyboard_event(s: &mut InputState, ev: InputEvent) {
    if ev.event_type != EV_KEY {
        return;
    }

    let down = ev.value != 0;

    match ev.code {
        KEY_LEFTSHIFT => {
            s.left_shift = down;
            return;
        }
        KEY_RIGHTSHIFT => {
            s.right_shift = down;
            return;
        }
        KEY_LEFTCTRL => {
            s.left_ctrl = down;
            return;
        }
        KEY_RIGHTCTRL => {
            s.right_ctrl = down;
            return;
        }
        KEY_LEFTALT => {
            s.left_alt = down;
            return;
        }
        KEY_RIGHTALT => {
            s.right_alt = down;
            return;
        }
        KEY_LEFTMETA => {
            s.left_meta = down;
            return;
        }
        KEY_RIGHTMETA => {
            s.right_meta = down;
            return;
        }
        KEY_CAPSLOCK if ev.value == 1 => {
            s.caps_lock = !s.caps_lock;
            return;
        }
        KEY_CAPSLOCK => return,
        _ => {}
    }

    // value 0 = release, 1 = press, 2 = repeat
    if ev.value == 0 {
        return;
    }

    let shift = s.left_shift || s.right_shift;
    let ctrl = s.left_ctrl || s.right_ctrl;
    let alt = s.left_alt || s.right_alt;
    let meta = s.left_meta || s.right_meta;

    let key = match ev.code {
        KEY_ENTER => Key::Enter,
        KEY_BACKSPACE => Key::Backspace,
        KEY_TAB => Key::Tab,
        KEY_ESC => Key::Escape,
        KEY_UP => Key::Up,
        KEY_DOWN => Key::Down,
        KEY_LEFT => Key::Left,
        KEY_RIGHT => Key::Right,
        KEY_HOME => Key::Home,
        KEY_END => Key::End,
        KEY_DELETE => Key::Delete,
        code => {
            let Some(byte) = key_to_ascii(code, shift, s.caps_lock) else {
                return;
            };
            Key::Char(byte)
        }
    };

    crate::monitor::handle_key(KeyEvent {
        key,
        ctrl,
        alt,
        shift,
        meta,
    });
}

pub unsafe fn init() {
    crate::uart_write("input: init begin\n");
    let s = state();

    for slot in 0..VIRTIO_MMIO_SLOTS {
        if s.pointer.is_some() && s.keyboard.is_some() {
            break;
        }

        let addr = VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE;
        let Some(header) = NonNull::new(addr as *mut VirtIOHeader) else {
            continue;
        };

        let transport: MmioTransport<'static> =
            match MmioTransport::new(header, VIRTIO_MMIO_STRIDE) {
                Ok(t) => t,
                Err(_) => continue,
            };

        if transport.device_type() != DeviceType::Input {
            continue;
        }

        crate::uart_write("input: VirtIO input transport found\n");
        crate::uart_write("input: constructing driver\n");

        let mut driver = match VirtIOInput::<KizunaHal, _>::new(transport) {
            Ok(d) => {
                crate::uart_write("input: driver constructed\n");
                d
            }
            Err(_) => {
                crate::uart_write("input: virtio device init failed\n");
                continue;
            }
        };

        crate::uart_write("input: querying ABS axes\n");
        let xinfo = driver.abs_info(ABS_X as u8);
        let yinfo = driver.abs_info(ABS_Y as u8);

        match (xinfo, yinfo) {
            (Ok(x), Ok(y)) if x.max > x.min && y.max > y.min && s.pointer.is_none() => {
                s.raw_x = x.min + (x.max - x.min) / 2;
                s.raw_y = y.min + (y.max - y.min) / 2;
                s.abs_x = x;
                s.abs_y = y;
                s.pointer = Some(driver);
                crate::uart_write("input: VirtIO tablet online\n");
            }
            _ if s.keyboard.is_none() => {
                s.keyboard = Some(driver);
                crate::uart_write("input: VirtIO keyboard online\n");
            }
            _ => {
                crate::uart_write("input: extra VirtIO input device ignored\n");
            }
        }
    }

    if s.pointer.is_some() {
        crate::desktop::pointer_update(
            crate::framebuffer::FB_W as i32 / 2,
            crate::framebuffer::FB_H as i32 / 2,
            false,
        );
    }

    if s.pointer.is_none() {
        crate::uart_write("input: no pointer device found\n");
    }
    if s.keyboard.is_none() {
        crate::uart_write("input: no keyboard device found\n");
    }

    crate::uart_write("input: init complete\n");
}

pub unsafe fn poll() {
    let s = state();

    // Drain the whole VirtIO batch first, then render the cursor once at the
    // newest position. Without this, a burst of tablet reports redraws the
    // cursor several times before QEMU even presents one host frame.
    let mut pointer_dirty = false;
    loop {
        let event = match s.pointer.as_mut() {
            Some(driver) => driver.pop_pending_event(),
            None => None,
        };
        let Some(event) = event else {
            break;
        };
        pointer_dirty |= handle_pointer_event(s, event);
    }

    if pointer_dirty {
        let x = scale_axis(s.raw_x, &s.abs_x, crate::framebuffer::FB_W);
        let y = scale_axis(s.raw_y, &s.abs_y, crate::framebuffer::FB_H);
        crate::desktop::pointer_event(
            x,
            y,
            s.left_down,
            s.right_down,
            s.middle_down,
        );
    }

    if let Some(driver) = s.pointer.as_mut() {
        let _ = driver.ack_interrupt();
    }

    loop {
        let event = match s.keyboard.as_mut() {
            Some(driver) => driver.pop_pending_event(),
            None => None,
        };
        let Some(event) = event else {
            break;
        };
        handle_keyboard_event(s, event);
    }

    if let Some(driver) = s.keyboard.as_mut() {
        let _ = driver.ack_interrupt();
    }
}

pub fn has_pointer() -> bool {
    unsafe { (*INPUT.0.get()).pointer.is_some() }
}

pub fn has_keyboard() -> bool {
    unsafe { (*INPUT.0.get()).keyboard.is_some() }
}
