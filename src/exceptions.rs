// KizunaOS v0.0.2 — aarch64 exception handling
#![allow(unsafe_op_in_unsafe_fn)]

use core::arch::global_asm;

const UART0_BASE: usize = 0x0900_0000;
const UART_DR: *mut u8 = UART0_BASE as *mut u8;
const UART_FR: *const u32 = (UART0_BASE + 0x18) as *const u32;
const UART_FR_TXFF: u32 = 1 << 5;

unsafe fn putb(b: u8) {
    while core::ptr::read_volatile(UART_FR) & UART_FR_TXFF != 0 {}
    core::ptr::write_volatile(UART_DR, b);
}

unsafe fn puts(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' { putb(b'\r'); }
        putb(b);
    }
}

unsafe fn put_hex64(v: u64) {
    puts("0x");
    let mut buf = [0u8; 16];
    for j in 0..16 {
        let nib = ((v >> ((15 - j) * 4)) & 0xf) as u8;
        buf[j] = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
    }
    for &b in buf.iter() { putb(b); }
}

fn ec_name(ec: u64) -> &'static str {
    match ec {
        0x00 => "Unknown reason",
        0x0e => "Illegal execution state",
        0x15 => "SVC (AArch64)",
        0x18 => "MSR/MRS/system trap",
        0x20 => "Instruction abort, lower EL",
        0x21 => "Instruction abort, same EL",
        0x22 => "PC alignment fault",
        0x24 => "Data abort, lower EL",
        0x25 => "Data abort, same EL",
        0x26 => "SP alignment fault",
        0x2c => "Trapped FP exception",
        0x30 => "Breakpoint, lower EL",
        0x31 => "Breakpoint, same EL",
        0x3c => "BRK instruction (AArch64)",
        _    => "(other)",
    }
}

#[unsafe(no_mangle)]
extern "C" fn rust_exception_handler(index: u64, esr: u64, elr: u64, far: u64, spsr: u64) -> ! {
    unsafe {
        let ec = (esr >> 26) & 0x3f;
        puts("\n======== KIZUNA EXCEPTION ========\n");
        puts("vector index : "); put_hex64(index);                puts("\n");
        puts("ESR_EL1      : "); put_hex64(esr);                  puts("\n");
        puts("  EC         : "); put_hex64(ec); puts("  "); puts(ec_name(ec)); puts("\n");
        puts("  IL         : "); put_hex64((esr >> 25) & 1);      puts("\n");
        puts("  ISS        : "); put_hex64(esr & 0x1ff_ffff);     puts("\n");
        puts("ELR_EL1      : "); put_hex64(elr);                  puts("\n");
        puts("FAR_EL1      : "); put_hex64(far);                  puts("\n");
        puts("SPSR_EL1     : "); put_hex64(spsr);                 puts("\n");
        puts("==================================\n");
        puts("halted.\n");
    }
    loop { unsafe { core::arch::asm!("wfe") } }
}

pub fn current_el() -> u64 {
    let el: u64;
    unsafe { core::arch::asm!("mrs {}, CurrentEL", out(reg) el, options(nostack, nomem)); }
    (el >> 2) & 0x3
}

pub unsafe fn report_el() {
    puts("CurrentEL = EL");
    putb(b'0' + (current_el() as u8));
    puts("\n");
}

pub unsafe fn init() {
    unsafe extern "C" { static __kizuna_vectors: u8; }
    let addr = &__kizuna_vectors as *const u8 as u64;
    core::arch::asm!("msr vbar_el1, {}", in(reg) addr, options(nostack, nomem));
    core::arch::asm!("isb", options(nostack, nomem));
}

global_asm!(r#"
.balign 0x800
.global __kizuna_vectors
__kizuna_vectors:
    .balign 0x80
    mov x0, #0
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #1
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #2
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #3
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #4
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #5
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #6
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #7
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #8
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #9
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #10
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #11
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #12
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #13
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #14
    b   __kizuna_exc_common
    .balign 0x80
    mov x0, #15
    b   __kizuna_exc_common

__kizuna_exc_common:
    mrs x1, esr_el1
    mrs x2, elr_el1
    mrs x3, far_el1
    mrs x4, spsr_el1
    b   rust_exception_handler
"#);
