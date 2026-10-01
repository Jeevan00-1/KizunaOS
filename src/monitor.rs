#![allow(unsafe_op_in_unsafe_fn)]
extern crate alloc;

use core::cell::UnsafeCell;

use crate::input::{Key, KeyEvent};

// KizunaOS interactive kernel monitor.
// Event-driven: UART and VirtIO keyboard both feed the same shell.

const UART_BASE: usize = 0x0900_0000;
const UART_DR: *mut u32 = UART_BASE as *mut u32;
const UART_FR: *const u32 = (UART_BASE + 0x18) as *const u32;
const UART_FR_TXFF: u32 = 1 << 5;
const UART_FR_RXFE: u32 = 1 << 4;

const HISTORY_CAP: usize = 8;

struct MonitorState {
    buf: [u8; 128],
    len: usize,
    cursor: usize,

    history: [[u8; 128]; HISTORY_CAP],
    history_len: [usize; HISTORY_CAP],
    history_count: usize,
    history_nav: usize,
}

impl MonitorState {
    const fn new() -> Self {
        Self {
            buf: [0; 128],
            len: 0,
            cursor: 0,
            history: [[0; 128]; HISTORY_CAP],
            history_len: [0; HISTORY_CAP],
            history_count: 0,
            history_nav: 0,
        }
    }
}

struct MonitorCell(UnsafeCell<MonitorState>);
unsafe impl Sync for MonitorCell {}
static MONITOR: MonitorCell = MonitorCell(UnsafeCell::new(MonitorState::new()));

fn raw_putc(b: u8) {
    unsafe {
        while core::ptr::read_volatile(UART_FR) & UART_FR_TXFF != 0 {}
        core::ptr::write_volatile(UART_DR, b as u32);
    }
}

fn raw_puts(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            raw_putc(b'\r');
        }
        raw_putc(b);
    }
}

fn putc(b: u8) {
    if b == b'\n' {
        raw_putc(b'\r');
    }
    raw_putc(b);

    unsafe {
        crate::desktop::console_putc(b);
    }
}

fn puts(s: &str) {
    for &b in s.as_bytes() {
        putc(b);
    }
}

fn put_hex64(v: u64) {
    puts("0x");
    for j in 0..16 {
        let nib = ((v >> ((15 - j) * 4)) & 0xf) as u8;
        putc(if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) });
    }
}

fn parse_hex(s: &[u8]) -> Option<u64> {
    let mut bytes = s;
    if bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] == b'x' || bytes[1] == b'X') {
        bytes = &bytes[2..];
    }
    if bytes.is_empty() {
        return None;
    }

    let mut val: u64 = 0;
    for &c in bytes {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        val = val.wrapping_mul(16).wrapping_add(d as u64);
    }

    Some(val)
}


fn move_visual_left(count: usize) {
    for _ in 0..count {
        raw_puts("\x1b[D");
        unsafe { crate::desktop::console_move_left(); }
    }
}

fn move_visual_right(count: usize) {
    for _ in 0..count {
        raw_puts("\x1b[C");
        unsafe { crate::desktop::console_move_right(); }
    }
}

fn erase_current_line(state: &mut MonitorState) {
    move_visual_left(state.cursor);

    for _ in 0..state.len {
        raw_putc(b' ');
        unsafe { crate::desktop::console_putc(b' '); }
    }

    move_visual_left(state.len);

    state.len = 0;
    state.cursor = 0;
    state.history_nav = 0;
}

fn replace_current_line(state: &mut MonitorState, bytes: &[u8]) {
    erase_current_line(state);

    let n = bytes.len().min(state.buf.len() - 1);
    state.buf[..n].copy_from_slice(&bytes[..n]);
    state.len = n;
    state.cursor = n;

    for &b in &state.buf[..n] {
        raw_putc(b);
        unsafe { crate::desktop::console_putc(b); }
    }
}

fn push_history(state: &mut MonitorState, line: &[u8]) {
    if line.is_empty() {
        state.history_nav = 0;
        return;
    }

    // Do not duplicate the newest command.
    if state.history_count > 0 {
        let newest_len = state.history_len[0];
        if newest_len == line.len() && state.history[0][..newest_len] == line[..] {
            state.history_nav = 0;
            return;
        }
    }

    for i in (1..HISTORY_CAP).rev() {
        state.history[i] = state.history[i - 1];
        state.history_len[i] = state.history_len[i - 1];
    }

    let n = line.len().min(state.history[0].len());
    state.history[0].fill(0);
    state.history[0][..n].copy_from_slice(&line[..n]);
    state.history_len[0] = n;
    state.history_count = (state.history_count + 1).min(HISTORY_CAP);
    state.history_nav = 0;
}

fn history_up(state: &mut MonitorState) {
    if state.history_count == 0 {
        return;
    }

    state.history_nav = (state.history_nav + 1).min(state.history_count);
    let index = state.history_nav - 1;
    let len = state.history_len[index];
    let mut line = [0u8; 128];
    line[..len].copy_from_slice(&state.history[index][..len]);
    replace_current_line(state, &line[..len]);
    state.history_nav = index + 1;
}

fn history_down(state: &mut MonitorState) {
    if state.history_nav == 0 {
        return;
    }

    state.history_nav -= 1;

    if state.history_nav == 0 {
        replace_current_line(state, &[]);
        return;
    }

    let index = state.history_nav - 1;
    let len = state.history_len[index];
    let mut line = [0u8; 128];
    line[..len].copy_from_slice(&state.history[index][..len]);
    replace_current_line(state, &line[..len]);
    state.history_nav = index + 1;
}

fn insert_byte(state: &mut MonitorState, byte: u8) {
    if state.len >= state.buf.len() - 1 {
        return;
    }

    for i in (state.cursor..state.len).rev() {
        state.buf[i + 1] = state.buf[i];
    }

    state.buf[state.cursor] = byte;
    state.len += 1;
    state.cursor += 1;
    state.history_nav = 0;

    let start = state.cursor - 1;
    for i in start..state.len {
        let b = state.buf[i];
        raw_putc(b);
        unsafe { crate::desktop::console_putc(b); }
    }

    let tail = state.len - state.cursor;
    move_visual_left(tail);
}

fn backspace(state: &mut MonitorState) {
    if state.cursor == 0 {
        return;
    }

    state.cursor -= 1;
    move_visual_left(1);

    for i in state.cursor..state.len - 1 {
        state.buf[i] = state.buf[i + 1];
    }
    state.len -= 1;

    for i in state.cursor..state.len {
        let b = state.buf[i];
        raw_putc(b);
        unsafe { crate::desktop::console_putc(b); }
    }

    raw_putc(b' ');
    unsafe { crate::desktop::console_putc(b' '); }

    move_visual_left((state.len - state.cursor) + 1);
    state.history_nav = 0;
}

fn delete_at_cursor(state: &mut MonitorState) {
    if state.cursor >= state.len {
        return;
    }

    for i in state.cursor..state.len - 1 {
        state.buf[i] = state.buf[i + 1];
    }
    state.len -= 1;

    for i in state.cursor..state.len {
        let b = state.buf[i];
        raw_putc(b);
        unsafe { crate::desktop::console_putc(b); }
    }

    raw_putc(b' ');
    unsafe { crate::desktop::console_putc(b' '); }

    move_visual_left((state.len - state.cursor) + 1);
    state.history_nav = 0;
}

fn current_el() -> u64 {
    let el: u64;
    unsafe {
        core::arch::asm!("mrs {}, CurrentEL", out(reg) el, options(nostack, nomem));
    }
    (el >> 2) & 0x3
}

fn cmd_help() {
    puts("kizuna monitor commands:\n");
    puts("  help              this list\n");
    puts("  clear             clear serial + graphical terminal\n");
    puts("  input             VirtIO input device status\n");
    puts("  el                show current exception level\n");
    puts("  regs              dump key system registers\n");
    puts("  peek <hex>        read 32 bits from an address\n");
    puts("  poke <hex> <hex>  write 32 bits to an address\n");
    puts("  fault             trigger a recoverable data abort\n");
    puts("  mem               show the known memory map\n");
    puts("  heap              heap allocator stats\n");
    puts("  alloctest         allocate a Vec and String\n");
    puts("  uaf               demonstrate freed-chunk reuse\n");
    puts("  poweroff / halt   power off the machine\n");
    puts("  reboot            restart the machine\n");
}

fn cmd_input() {
    puts("VirtIO input:\n");
    puts("  keyboard : ");
    puts(if crate::input::has_keyboard() { "online\n" } else { "missing\n" });
    puts("  tablet   : ");
    puts(if crate::input::has_pointer() { "online\n" } else { "missing\n" });
    puts("  terminal : ");
    puts(if crate::desktop::terminal_focused() { "focused\n" } else { "unfocused\n" });
    puts("keys: arrows edit/history, Ctrl+A/E, Ctrl+U, Ctrl+C, Ctrl+L\n");
    puts("global: Ctrl+Alt+T focuses Terminal\n");
}

fn cmd_regs() {
    let (sp, vbar, sctlr): (u64, u64, u64);
    unsafe {
        core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack));
        core::arch::asm!("mrs {}, vbar_el1", out(reg) vbar, options(nomem, nostack));
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr, options(nomem, nostack));
    }

    puts("SP        : ");
    put_hex64(sp);
    puts("\n");
    puts("VBAR_EL1  : ");
    put_hex64(vbar);
    puts("\n");
    puts("SCTLR_EL1 : ");
    put_hex64(sctlr);
    puts("\n");
}

fn cmd_mem() {
    puts("known memory map (QEMU virt):\n");
    puts("  0x09000000  PL011 UART\n");
    puts("  0x09020000  fw_cfg\n");
    puts("  0x0a000000  VirtIO MMIO transport bank\n");
    puts("  0x40000000  RAM base\n");
    puts("  0x40100000  kernel load address\n");
}

fn cmd_peek(arg: &[u8]) {
    match parse_hex(arg) {
        Some(addr) => {
            let v = unsafe { core::ptr::read_volatile(addr as *const u32) };
            puts("[");
            put_hex64(addr);
            puts("] = ");
            put_hex64(v as u64);
            puts("\n");
        }
        None => puts("usage: peek <hexaddr>\n"),
    }
}

fn cmd_poke(a: &[u8], b: &[u8]) {
    match (parse_hex(a), parse_hex(b)) {
        (Some(addr), Some(val)) => {
            unsafe {
                core::ptr::write_volatile(addr as *mut u32, val as u32);
            }
            puts("wrote ");
            put_hex64(val);
            puts(" -> ");
            put_hex64(addr);
            puts("\n");
        }
        _ => puts("usage: poke <hexaddr> <hexval>\n"),
    }
}

fn cmd_fault() {
    puts("triggering one expected data abort at 0xffff0000dead0000...\n");
    crate::exceptions::expect_data_abort();
    let p = 0xffff_0000_dead_0000usize as *const u8;
    let _x = unsafe { core::ptr::read_volatile(p) };
    puts("...and we're back. monitor survived the expected fault.\n");
}

fn tokenize(line: &[u8]) -> ([&[u8]; 3], usize) {
    let mut toks: [&[u8]; 3] = [&[], &[], &[]];
    let mut n = 0;
    let mut i = 0;

    while n < 3 {
        while i < line.len() && line[i] == b' ' {
            i += 1;
        }
        if i >= line.len() {
            break;
        }

        let start = i;
        while i < line.len() && line[i] != b' ' {
            i += 1;
        }

        toks[n] = &line[start..i];
        n += 1;
    }

    (toks, n)
}

fn execute_line(line: &[u8]) {
    let (toks, n) = tokenize(line);
    if n == 0 {
        return;
    }

    match toks[0] {
        b"help" => cmd_help(),
        b"clear" => cmd_clear(),
        b"input" => cmd_input(),
        b"el" => {
            puts("CurrentEL = EL");
            putc(b'0' + current_el() as u8);
            puts("\n");
        }
        b"regs" => cmd_regs(),
        b"mem" => cmd_mem(),
        b"peek" => cmd_peek(toks[1]),
        b"poke" => cmd_poke(toks[1], toks[2]),
        b"fault" => cmd_fault(),
        b"heap" => cmd_heap(),
        b"alloctest" => cmd_alloctest(),
        b"uaf" => cmd_uaf(),
        b"poweroff" | b"halt" => cmd_poweroff(),
        b"reboot" => cmd_reboot(),
        _ => {
            puts("unknown command: ");
            puts(core::str::from_utf8(toks[0]).unwrap_or("?"));
            puts("\n");
        }
    }
}

fn prompt() {
    puts("kizuna> ");
}

pub fn handle_key(event: KeyEvent) {
    // Global Arch-style terminal focus shortcut.
    if event.ctrl && event.alt {
        if let Key::Char(c) = event.key {
            if c.to_ascii_lowercase() == b't' {
                unsafe { crate::desktop::focus_terminal(); }
                return;
            }
        }
    }

    if !crate::desktop::terminal_focused() {
        return;
    }

    unsafe {
        let state = &mut *MONITOR.0.get();

        if event.ctrl {
            if let Key::Char(c) = event.key {
                match c.to_ascii_lowercase() {
                    b'c' => {
                        erase_current_line(state);
                        puts("^C\n");
                        prompt();
                        return;
                    }
                    b'l' => {
                        state.len = 0;
                        state.cursor = 0;
                        state.history_nav = 0;
                        cmd_clear();
                        prompt();
                        return;
                    }
                    b'u' => {
                        erase_current_line(state);
                        return;
                    }
                    b'a' => {
                        move_visual_left(state.cursor);
                        state.cursor = 0;
                        return;
                    }
                    b'e' => {
                        let n = state.len - state.cursor;
                        move_visual_right(n);
                        state.cursor = state.len;
                        return;
                    }
                    _ => {}
                }
            }
        }

        match event.key {
            Key::Char(c) => {
                if !event.ctrl && !event.alt && !event.meta {
                    insert_byte(state, c);
                }
            }
            Key::Enter => {
                raw_puts("\r\n");
                crate::desktop::console_putc(b'\n');

                let len = state.len;
                let mut line = [0u8; 128];
                line[..len].copy_from_slice(&state.buf[..len]);
                push_history(state, &line[..len]);

                state.len = 0;
                state.cursor = 0;
                state.history_nav = 0;

                execute_line(&line[..len]);
                prompt();
            }
            Key::Backspace => backspace(state),
            Key::Delete => delete_at_cursor(state),
            Key::Tab => {
                for _ in 0..4 {
                    insert_byte(state, b' ');
                }
            }
            Key::Escape => erase_current_line(state),
            Key::Up => history_up(state),
            Key::Down => history_down(state),
            Key::Left => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    move_visual_left(1);
                }
            }
            Key::Right => {
                if state.cursor < state.len {
                    state.cursor += 1;
                    move_visual_right(1);
                }
            }
            Key::Home => {
                move_visual_left(state.cursor);
                state.cursor = 0;
            }
            Key::End => {
                let n = state.len - state.cursor;
                move_visual_right(n);
                state.cursor = state.len;
            }
        }
    }
}

pub fn feed_input(c: u8) {
    let event = match c {
        b'\r' | b'\n' => KeyEvent {
            key: Key::Enter,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        },
        0x7f | 0x08 => KeyEvent {
            key: Key::Backspace,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        },
        b'\t' => KeyEvent {
            key: Key::Tab,
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        },
        0x20..=0x7e => KeyEvent {
            key: Key::Char(c),
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        },
        _ => return,
    };

    handle_key(event);
}

fn poll_uart() {
    for _ in 0..64 {
        let empty = unsafe { core::ptr::read_volatile(UART_FR) & UART_FR_RXFE != 0 };
        if empty {
            break;
        }

        let c = unsafe { (core::ptr::read_volatile(UART_DR) & 0xff) as u8 };
        feed_input(c);
    }
}

pub fn run() -> ! {
    puts("\nKizuna event shell online. click the QEMU window and type directly.\n");
    prompt();

    loop {
        poll_uart();
        unsafe {
            crate::input::poll();
            core::arch::asm!("yield", options(nomem, nostack));
        }
    }
}

fn cmd_poweroff() -> ! {
    puts("kizuna: powering off.\n");
    let fn_id: u64 = 0x8400_0008;
    unsafe {
        core::arch::asm!("hvc #0", in("x0") fn_id, options(noreturn));
    }
}

fn cmd_reboot() -> ! {
    puts("kizuna: rebooting.\n");
    let fn_id: u64 = 0x8400_0009;
    unsafe {
        core::arch::asm!("hvc #0", in("x0") fn_id, options(noreturn));
    }
}

fn cmd_heap() {
    let a = &crate::heap::ALLOCATOR;
    puts("arena base : ");
    put_hex64(a.base() as u64);
    puts("\n");
    puts("total      : ");
    put_hex64(a.total() as u64);
    puts(" bytes\n");
    puts("used(bump) : ");
    put_hex64(a.used() as u64);
    puts(" bytes\n");
    puts("allocs     : ");
    put_hex64(a.allocs() as u64);
    puts("\n");
    puts("frees      : ");
    put_hex64(a.frees() as u64);
    puts("\n");
    puts("reuses     : ");
    put_hex64(a.reuses() as u64);
    puts("  <- freed chunks recycled\n");
}

fn cmd_alloctest() {
    use alloc::string::String;
    use alloc::vec::Vec;

    puts("allocating a Vec<u64> and pushing 8 values...\n");
    let mut v: Vec<u64> = Vec::new();
    for i in 0..8 {
        v.push(0x1000 + i);
    }

    puts("vec[0] = ");
    put_hex64(v[0]);
    puts("   vec[7] = ");
    put_hex64(v[7]);
    puts("\n");
    puts("vec backing ptr = ");
    put_hex64(v.as_ptr() as u64);
    puts("\n");

    let mut s = String::new();
    s.push_str("kizuna heap works");
    puts("string built on heap: ");
    puts(&s);
    puts("\n");
}

fn cmd_clear() {
    raw_puts("\x1b[2J\x1b[H");
    unsafe {
        crate::desktop::console_clear();
    }
}

fn cmd_uaf() {
    use alloc::boxed::Box;

    puts("--- UAF substrate demo ---\n");

    let a: Box<[u64; 4]> = Box::new([0xAAAA_AAAA; 4]);
    let addr_a = a.as_ptr() as u64;
    puts("alloc A  @ ");
    put_hex64(addr_a);
    puts("  (filled with 0xAAAAAAAA)\n");

    drop(a);
    puts("free  A  -> chunk pushed to free list\n");

    let b: Box<[u64; 4]> = Box::new([0xBBBB_BBBB; 4]);
    let addr_b = b.as_ptr() as u64;
    puts("alloc B  @ ");
    put_hex64(addr_b);
    puts("  (filled with 0xBBBBBBBB)\n");

    if addr_a == addr_b {
        puts(">>> SAME ADDRESS. B reused A's freed memory.\n");
        puts(">>> stale A pointer now aliases B: the UAF substrate.\n");
    } else {
        puts(">>> different address (bin/timing). run again.\n");
    }

    let leaked = unsafe { core::ptr::read_volatile(addr_a as *const u64) };
    puts("peek old-A addr = ");
    put_hex64(leaked);
    puts("\n");

    drop(b);
}
