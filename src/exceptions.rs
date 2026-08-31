// KizunaOS v0.0.3 — exception handling with a full trap frame + recovery
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
    for j in 0..16 {
        let nib = ((v >> ((15 - j) * 4)) & 0xf) as u8;
        putb(if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) });
    }
}

fn ec_name(ec: u64) -> &'static str {
    match ec {
        0x15 => "SVC (syscall)",
        0x20 => "Instruction abort, lower EL",
        0x21 => "Instruction abort, same EL",
        0x24 => "Data abort, lower EL",
        0x25 => "Data abort, same EL",
        0x3c => "BRK instruction",
        _    => "(other)",
    }
}

/// The saved CPU state at the moment of a fault. This IS our pt_regs.
/// Field offsets MUST match the store/load order in the assembly below.
#[repr(C)]
pub struct TrapFrame {
    pub x: [u64; 31], // x0..x30   offsets 0x00..0xf0
    pub elr: u64,     // 0xf8  — the address we return to
    pub spsr: u64,    // 0x100 — processor state to restore
}

#[unsafe(no_mangle)]
extern "C" fn rust_exception_handler(index: u64, frame: *mut TrapFrame) {
    let f = unsafe { &mut *frame };
    let esr: u64;
    let far: u64;
    unsafe {
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr, options(nomem, nostack));
        core::arch::asm!("mrs {}, far_el1", out(reg) far, options(nomem, nostack));
    }
    let ec = (esr >> 26) & 0x3f;

    unsafe {
        puts("\n---- kizuna trap ----\n");
        puts("index : "); put_hex64(index); puts("\n");
        puts("EC    : "); put_hex64(ec); puts("  "); puts(ec_name(ec)); puts("\n");
        puts("ELR   : "); put_hex64(f.elr); puts("\n");
        puts("FAR   : "); put_hex64(far); puts("\n");
        puts("x0    : "); put_hex64(f.x[0]); puts("   x30: "); put_hex64(f.x[30]); puts("\n");
    }

    // ---- RECOVERY ----
    // For a synchronous fault we understand, skip the 4-byte faulting
    // instruction and RETURN. This turns a crash into a survivable event —
    // the defining behaviour of an operating system.
    // (A real OS would map the page and RETRY; skipping is the honest demo
    //  of "return from exception" — real fault resolution is v0.0.6/MMU.)
    if ec == 0x25 || ec == 0x24 || ec == 0x3c {
        f.elr += 4;
        unsafe { puts("recover: skip faulting insn, resume\n---------------------\n"); }
    } else {
        unsafe { puts("recover: UNHANDLED — halting\n---------------------\n"); }
        loop { unsafe { core::arch::asm!("wfe") } }
    }
    // normal return -> assembly restores the frame and performs `eret`
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

// 16 vector entries. Each saves x0/x1, records its index, and jumps to the
// common save/dispatch path. Unrolled (no assembler macros) for zero surprises.
global_asm!(r#"
.balign 0x800
.global __kizuna_vectors
__kizuna_vectors:
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #0
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #1
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #2
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #3
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #4
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #5
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #6
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #7
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #8
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #9
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #10
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #11
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #12
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #13
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #14
    b   __kizuna_save_dispatch
    .balign 0x80
    sub sp, sp, #0x110
    stp x0, x1, [sp, #0x00]
    mov x0, #15
    b   __kizuna_save_dispatch

__kizuna_save_dispatch:
    // x0 = vector index; x1's original already saved; sp = frame base
    stp x2,  x3,  [sp, #0x10]
    stp x4,  x5,  [sp, #0x20]
    stp x6,  x7,  [sp, #0x30]
    stp x8,  x9,  [sp, #0x40]
    stp x10, x11, [sp, #0x50]
    stp x12, x13, [sp, #0x60]
    stp x14, x15, [sp, #0x70]
    stp x16, x17, [sp, #0x80]
    stp x18, x19, [sp, #0x90]
    stp x20, x21, [sp, #0xa0]
    stp x22, x23, [sp, #0xb0]
    stp x24, x25, [sp, #0xc0]
    stp x26, x27, [sp, #0xd0]
    stp x28, x29, [sp, #0xe0]
    str x30,      [sp, #0xf0]
    mrs x9,  elr_el1
    mrs x10, spsr_el1
    stp x9, x10,  [sp, #0xf8]

    mov x1, sp                 // arg2 = pointer to the trap frame
    bl  rust_exception_handler

    // ---- restore everything and return ----
    ldp x9, x10,  [sp, #0xf8]  // (possibly modified) elr, spsr
    msr elr_el1,  x9
    msr spsr_el1, x10
    ldp x0,  x1,  [sp, #0x00]
    ldp x2,  x3,  [sp, #0x10]
    ldp x4,  x5,  [sp, #0x20]
    ldp x6,  x7,  [sp, #0x30]
    ldp x8,  x9,  [sp, #0x40]
    ldp x10, x11, [sp, #0x50]
    ldp x12, x13, [sp, #0x60]
    ldp x14, x15, [sp, #0x70]
    ldp x16, x17, [sp, #0x80]
    ldp x18, x19, [sp, #0x90]
    ldp x20, x21, [sp, #0xa0]
    ldp x22, x23, [sp, #0xb0]
    ldp x24, x25, [sp, #0xc0]
    ldp x26, x27, [sp, #0xd0]
    ldp x28, x29, [sp, #0xe0]
    ldr x30,      [sp, #0xf0]
    add sp, sp,   #0x110
    eret
"#);
