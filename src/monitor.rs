#![allow(unsafe_op_in_unsafe_fn)]
extern crate alloc;
// KizunaOS interactive kernel monitor.
// v0.1.0 desktop-alpha mirrors monitor I/O into the graphical terminal.

const UART_BASE: usize = 0x0900_0000;
const UART_DR: *mut u32 = UART_BASE as *mut u32;
const UART_FR: *const u32 = (UART_BASE + 0x18) as *const u32;
const UART_FR_TXFF: u32 = 1 << 5;
const UART_FR_RXFE: u32 = 1 << 4;

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
        putc(if nib < 10 {
            b'0' + nib
        } else {
            b'a' + (nib - 10)
        });
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
    puts("triggering data abort at 0xffff0000dead0000...\n");
    let p = 0xffff_0000_dead_0000usize as *const u8;
    let _x = unsafe { core::ptr::read_volatile(p) };
    puts("...and we're back. monitor survived the fault.\n");
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

pub fn run() -> ! {
    puts("\nKizuna monitor attached to desktop terminal. type 'help'.\n");
    let mut buf = [0u8; 128];

    loop {
        puts("kizuna> ");
        let mut len = 0;

        loop {
            let c = getc();
            match c {
                b'\r' | b'\n' => {
                    putc(b'\n');
                    break;
                }
                0x7f | 0x08 => {
                    if len > 0 {
                        len -= 1;
                        raw_puts("\x08 \x08");
                        unsafe {
                            crate::desktop::console_putc(0x08);
                        }
                    }
                }
                _ => {
                    if len < buf.len() - 1 {
                        buf[len] = c;
                        len += 1;
                        putc(c);
                    }
                }
            }
        }

        let (toks, n) = tokenize(&buf[..len]);
        if n == 0 {
            continue;
        }

        match toks[0] {
            b"help" => cmd_help(),
            b"clear" => cmd_clear(),
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
