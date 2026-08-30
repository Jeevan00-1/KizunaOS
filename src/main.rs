#![no_std]
#![no_main]

use core::{
    arch::{asm, global_asm},
    panic::PanicInfo,
    ptr::{read_volatile, write_volatile},
};

/*
 * This is the first code the CPU executes.
 *
 * There is no macOS.
 * There is no Linux.
 * There isn't even a stack yet.
 *
 * So we create one ourselves, then enter Rust.
 */
global_asm!(
    r#"
    .section .text.boot
    .global _start
    .type _start, %function

_start:
    // Enable floating point / Advanced SIMD at EL1.
    mrs  x0, cpacr_el1
    mov  x1, #0x300000
    orr  x0, x0, x1
    msr  cpacr_el1, x0
    isb

    // Establish our kernel stack.
    adrp x0, __stack_top
    add  x0, x0, :lo12:__stack_top
    mov  sp, x0

    bl rust_main

1:
    wfe
    b 1b
"#
);

/*
 * QEMU's ARM virt machine provides a PL011 UART.
 *
 * For our first milestone we're using its conventional
 * virtual address directly.
 */
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
        if byte == b'\n' {
            uart_putc(b'\r');
        }

        uart_putc(byte);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    uart_write("\n");
    uart_write("================================\n");
    uart_write("          KIZUNA OS\n");
    uart_write("================================\n");
    uart_write("\n");
    uart_write("AArch64 Kernel v0.0.1\n");
    uart_write("\n");
    uart_write("boot: ok\n");
    uart_write("architecture: arm64\n");
    uart_write("kernel: online\n");
    uart_write("\n");
    uart_write("Welcome to Kizuna.\n");
    uart_write("\n");

    loop {
        unsafe {
            asm!("wfe");
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    uart_write("\nKERNEL PANIC\n");

    loop {
        unsafe {
            asm!("wfe");
        }
    }
}
