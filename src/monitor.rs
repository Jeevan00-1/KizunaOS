// KizunaOS v0.0.4 — interactive kernel monitor over the UART
#![allow(unsafe_op_in_unsafe_fn)]

const UART_BASE: usize = 0x0900_0000;
const UART_DR: *mut u32 = UART_BASE as *mut u32;
const UART_FR: *const u32 = (UART_BASE + 0x18) as *const u32;
const UART_FR_TXFF: u32 = 1 << 5; // transmit FIFO full
const UART_FR_RXFE: u32 = 1 << 4; // receive FIFO empty

fn putc(b: u8) {
    unsafe {
        while core::ptr::read_volatile(UART_FR) & UART_FR_TXFF != 0 {}
        core::ptr::write_volatile(UART_DR, b as u32);
    }
}
fn puts(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' { putc(b'\r'); }
        putc(b);
    }
}
/// Blocking read of one byte from the UART. THIS is our first input.
fn getc() -> u8 {
    unsafe {
        while core::ptr::read_volatile(UART_FR) & UART_FR_RXFE != 0 {}
        (core::ptr::read_volatile(UART_DR) & 0xff) as u8
    }
}

fn put_hex64(v: u64) {
    puts("0x");
    for j in 0..16 {
        let nib = ((v >> ((15 - j) * 4)) & 0xf) as u8;
        putc(if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) });
    }
}

/// Parse a hex string like "40100000" or "0x40100000" into a u64.
fn parse_hex(s: &[u8]) -> Option<u64> {
    let mut bytes = s;
    if bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] == b'x' || bytes[1] == b'X') {
        bytes = &bytes[2..];
    }
    if bytes.is_empty() { return None; }
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

fn current_el() -> u64 {
    let el: u64;
    unsafe { core::arch::asm!("mrs {}, CurrentEL", out(reg) el, options(nostack, nomem)); }
    (el >> 2) & 0x3
}

fn cmd_help() {
    puts("kizuna monitor commands:\n");
    puts("  help              this list\n");
    puts("  el                show current exception level\n");
    puts("  regs              dump a few key system registers\n");
    puts("  peek <hex>        read 32 bits from an address\n");
    puts("  poke <hex> <hex>  write 32 bits to an address\n");
    puts("  fault             trigger a data abort (v0.0.3 recovers)\n");
    puts("  mem               show the known memory map\n");
    puts("  poweroff / halt   power off the machine\n");
    puts("  reboot            restart the machine\n");
}

fn cmd_regs() {
    let (mut sp, mut vbar, mut sctlr): (u64, u64, u64);
    unsafe {
        core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack));
        core::arch::asm!("mrs {}, vbar_el1", out(reg) vbar, options(nomem, nostack));
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr, options(nomem, nostack));
    }
    puts("SP        : "); put_hex64(sp);   puts("\n");
    puts("VBAR_EL1  : "); put_hex64(vbar); puts("\n");
    puts("SCTLR_EL1 : "); put_hex64(sctlr); puts("\n");
}

fn cmd_mem() {
    puts("known memory map (QEMU virt):\n");
    puts("  0x09000000  PL011 UART\n");
    puts("  0x40000000  RAM base\n");
    puts("  0x40100000  kernel load address\n");
}

fn cmd_peek(arg: &[u8]) {
    match parse_hex(arg) {
        Some(addr) => {
            let v = unsafe { core::ptr::read_volatile(addr as *const u32) };
            puts("["); put_hex64(addr); puts("] = "); put_hex64(v as u64); puts("\n");
        }
        None => puts("usage: peek <hexaddr>\n"),
    }
}

fn cmd_poke(a: &[u8], b: &[u8]) {
    match (parse_hex(a), parse_hex(b)) {
        (Some(addr), Some(val)) => {
            unsafe { core::ptr::write_volatile(addr as *mut u32, val as u32); }
            puts("wrote "); put_hex64(val); puts(" -> "); put_hex64(addr); puts("\n");
        }
        _ => puts("usage: poke <hexaddr> <hexval>\n"),
    }
}

fn cmd_fault() {
    puts("triggering data abort at 0xffff0000dead0000...\n");
    let p = 0xffff_0000_dead_0000usize as *const u8;
    let _x = unsafe { core::ptr::read_volatile(p) };
    puts("...and we're back. the monitor survived a fault.\n");
}

/// Split a line into up to 3 whitespace-separated tokens.
fn tokenize(line: &[u8]) -> ([&[u8]; 3], usize) {
    let mut toks: [&[u8]; 3] = [&[], &[], &[]];
    let mut n = 0;
    let mut i = 0;
    while i < line.len() && n < 3 {
        while i < line.len() && line[i] == b' ' { i += 1; }
        let start = i;
        while i < line.len() && line[i] != b' ' { i += 1; }
        if i > start { toks[n] = &line[start..i]; n += 1; }
    }
    (toks, n)
}

pub fn run() -> ! {
    puts("\nKizuna monitor. type 'help'.\n");
    let mut buf = [0u8; 128];

    loop {
        puts("kizuna> ");
        let mut len = 0;

        // read a line, with backspace handling and echo
        loop {
            let c = getc();
            match c {
                b'\r' | b'\n' => { putc(b'\r'); putc(b'\n'); break; }
                0x7f | 0x08 => { // backspace / delete
                    if len > 0 { len -= 1; puts("\x08 \x08"); }
                }
                _ => {
                    if len < buf.len() - 1 {
                        buf[len] = c;
                        len += 1;
                        putc(c); // echo
                    }
                }
            }
        }

        let (toks, n) = tokenize(&buf[..len]);
        if n == 0 { continue; }

        match toks[0] {
            b"help" => cmd_help(),
            b"el"   => { puts("CurrentEL = EL"); putc(b'0' + current_el() as u8); puts("\n"); }
            b"regs" => cmd_regs(),
            b"mem"  => cmd_mem(),
            b"peek" => cmd_peek(toks[1]),
            b"poke" => cmd_poke(toks[1], toks[2]),
            b"fault"=> cmd_fault(),
            b"poweroff" | b"halt" => cmd_poweroff(),
            b"reboot" => cmd_reboot(),
            _ => { puts("unknown command: "); puts(core::str::from_utf8(toks[0]).unwrap_or("?")); puts("\n"); }
        }
    }
}


/// PSCI SYSTEM_OFF — cleanly powers off the QEMU virt machine.
fn cmd_poweroff() -> ! {
    puts("kizuna: powering off.\n");
    let fn_id: u64 = 0x8400_0008;
    unsafe {
        core::arch::asm!("hvc #0", in("x0") fn_id, options(noreturn));
    }
}

/// PSCI SYSTEM_RESET — reboots the machine.
fn cmd_reboot() -> ! {
    puts("kizuna: rebooting.\n");
    let fn_id: u64 = 0x8400_0009;
    unsafe {
        core::arch::asm!("hvc #0", in("x0") fn_id, options(noreturn));
    }
}
