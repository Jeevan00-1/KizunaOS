#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

mod exceptions;

use core::{
    arch::{asm, global_asm},
    panic::PanicInfo,
    ptr::{read_volatile, write_volatile},
};

global_asm!(
    r#"
    .section .text.boot
    .global _start
    .type _start, %function
_start:
    mrs  x0, cpacr_el1
    mov  x1, #0x300000
    orr  x0, x0, x1
    msr  cpacr_el1, x0
    isb
    adrp x0, __stack_top
    add  x0, x0, :lo12:__stack_top
    mov  sp, x0
    bl rust_main
1:
    wfe
    b 1b
"#
);

const UART_BASE: usize = 0x0900_0000;
const UART_DR: usize = UART_BASE;
const UART_FR: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;

fn uart_putc(byte: u8) {
    unsafe {
        while read_volatile(UART_FR as *const u32) & UART_TX_FULL != 0 {}
        write_volatile(UART_DR as *mut u32, byte as u32);
    }
}
fn uart_write(text: &str) {
    for byte in text.bytes() {
        if byte == b'\n' { uart_putc(b'\r'); }
        uart_putc(byte);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    uart_write("\n");
    uart_write("================================\n");
    uart_write("          KIZUNA OS\n");
    uart_write("================================\n");
    uart_write("\nAArch64 Kernel v0.0.3\n\n");
    uart_write("boot: ok\n");

    unsafe {
        exceptions::report_el();
        exceptions::init();
        uart_write("vectors: installed\n");

        uart_write("\n[test 1] deliberate data abort...\n");
        let p = 0xffff_0000_dead_0000usize as *const u8;
        let _x = read_volatile(p);
        // In v0.0.2 the kernel HALTED here and the next line never printed.
        // In v0.0.3 the handler skips the faulting insn and RETURNS:
        uart_write(">>> SURVIVED. Kizuna caught the fault and kept running.\n");

        uart_write("\n[test 2] another fault, to prove it wasn't luck...\n");
        let q = 0xffff_0000_cafe_0000usize as *const u8;
        let _y = read_volatile(q);
        uart_write(">>> SURVIVED AGAIN. This is an OS: it recovers.\n");
    }

    uart_write("\nboot complete. entering idle loop.\n");
    loop { unsafe { asm!("wfe"); } }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    uart_write("\nKERNEL PANIC\n");
    loop { unsafe { asm!("wfe"); } }
}
