// KizunaOS early AArch64 MMU setup.
//
// Why this exists so early:
// With Stage-1 translation disabled, Arm treats EL1 data accesses as
// Device-nGnRnE. That is sufficient for very early bring-up, but it is not a
// valid long-term environment for normal RAM, atomics, VirtIO rings, heaps,
// or a graphical OS. HVF exposes this much more faithfully than TCG.
//
// We identity-map:
//   0x0000_0000..0x3fff_ffff  Device-nGnRnE (QEMU virt MMIO / flash / PCI)
//   0x4000_0000..0x7fff_ffff  Normal WB, Inner Shareable (RAM)
//
// A 39-bit VA regime with 4 KiB granules starts at level 1, so two 1 GiB
// block descriptors are enough for the current QEMU virt machine.
#![allow(unsafe_op_in_unsafe_fn)]

use core::arch::asm;

const DESC_BLOCK: u64 = 0b01;
const AF: u64 = 1 << 10;
const SH_OUTER: u64 = 0b10 << 8;
const SH_INNER: u64 = 0b11 << 8;
const ATTR_DEVICE: u64 = 0 << 2;
const ATTR_NORMAL: u64 = 1 << 2;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;

const DEVICE_1G: u64 =
    DESC_BLOCK | AF | SH_OUTER | ATTR_DEVICE | PXN | UXN;

const RAM_1G: u64 =
    0x4000_0000 | DESC_BLOCK | AF | SH_INNER | ATTR_NORMAL;

const fn l1_table() -> [u64; 512] {
    let mut table = [0u64; 512];
    table[0] = DEVICE_1G;
    table[1] = RAM_1G;
    table
}

#[repr(C, align(4096))]
struct PageTable([u64; 512]);

// Force this out of BSS: the table must already contain valid descriptors
// before rust_main zeroes BSS.
#[unsafe(link_section = ".data.mmu")]
static mut L1_TABLE: PageTable = PageTable(l1_table());

#[inline(always)]
unsafe fn read_sctlr() -> u64 {
    let value: u64;
    asm!("mrs {0}, sctlr_el1", out(reg) value, options(nostack, nomem));
    value
}

#[inline(always)]
pub fn enabled() -> bool {
    unsafe { read_sctlr() & 1 != 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kizuna_mmu_early_init() {
    // AttrIdx 0 = Device-nGnRnE (0x00)
    // AttrIdx 1 = Normal WB/RA/WA inner+outer (0xff)
    let mair: u64 = 0x00 | (0xff << 8);

    // 39-bit TTBR0 VA space, 4 KiB granule.
    // T0SZ = 25
    // IRGN0/ORGN0 = WB RA/WA
    // SH0 = Inner Shareable
    // EPD1 = disable TTBR1 walks for now.
    let mmfr0: u64;
    asm!("mrs {0}, id_aa64mmfr0_el1", out(reg) mmfr0, options(nostack, nomem));

    let parange = mmfr0 & 0xf;
    let ips = if parange <= 6 { parange } else { 5 };

    let tcr: u64 =
        25
        | (0b01 << 8)
        | (0b01 << 10)
        | (0b11 << 12)
        | (16 << 16)
        | (1 << 23)
        | (ips << 32);

    let ttbr0 = core::ptr::addr_of!(L1_TABLE) as u64;

    asm!("msr mair_el1, {0}", in(reg) mair, options(nostack));
    asm!("msr tcr_el1, {0}", in(reg) tcr, options(nostack));
    asm!("msr ttbr0_el1, {0}", in(reg) ttbr0, options(nostack));

    asm!("dsb ish", options(nostack));
    asm!("isb", options(nostack));
    asm!("tlbi vmalle1", options(nostack));
    asm!("dsb ish", options(nostack));
    asm!("isb", options(nostack));

    // No stale instruction lines from the pre-MMU regime.
    asm!("ic iallu", options(nostack));
    asm!("dsb ish", options(nostack));
    asm!("isb", options(nostack));

    let mut sctlr = read_sctlr();
    sctlr |= (1 << 0) | (1 << 2) | (1 << 12); // M, C, I
    asm!("msr sctlr_el1, {0}", in(reg) sctlr, options(nostack));
    asm!("isb", options(nostack));
}

pub fn report() {
    if enabled() {
        crate::uart_write("mmu: identity map online; RAM=Normal-WB, MMIO=Device\n");
    } else {
        crate::uart_write("mmu: FAILED to enable\n");
    }
}
