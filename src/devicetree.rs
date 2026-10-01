// Minimal Flattened Device Tree (FDT) reader for QEMU's ARM virt machine.
//
// QEMU only guarantees the RAM/flash locations on the 'virt' board. Device
// addresses must be discovered from the DTB. For an ELF bare-metal kernel,
// QEMU places the DTB at the beginning of RAM (0x4000_0000).
#![allow(unsafe_op_in_unsafe_fn)]

const DTB_ADDR: usize = 0x4000_0000;
const FDT_MAGIC: u32 = 0xd00d_feed;

const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;

pub const MAX_VIRTIO_MMIO: usize = 32;

#[derive(Clone, Copy)]
pub struct MmioRegion {
    pub base: usize,
    pub size: usize,
}

impl MmioRegion {
    const EMPTY: Self = Self { base: 0, size: 0 };
}

#[derive(Clone, Copy)]
struct NodeState {
    virtio_mmio: bool,
    reg_base: usize,
    reg_size: usize,
    have_reg: bool,
}

impl NodeState {
    const EMPTY: Self = Self {
        virtio_mmio: false,
        reg_base: 0,
        reg_size: 0,
        have_reg: false,
    };
}

#[inline(always)]
unsafe fn be32(base: *const u8, off: usize) -> u32 {
    u32::from_be(core::ptr::read_unaligned(base.add(off) as *const u32))
}

#[inline(always)]
unsafe fn be64_cells(data: *const u8, cells: u32) -> u64 {
    match cells {
        0 => 0,
        1 => be32(data, 0) as u64,
        _ => ((be32(data, 0) as u64) << 32) | be32(data, 4) as u64,
    }
}

#[inline(always)]
fn align4(v: usize) -> usize {
    (v + 3) & !3
}

unsafe fn cstr_eq(base: *const u8, max: usize, wanted: &[u8]) -> bool {
    if wanted.len() >= max {
        return false;
    }

    for (i, &b) in wanted.iter().enumerate() {
        if *base.add(i) != b {
            return false;
        }
    }

    *base.add(wanted.len()) == 0
}

unsafe fn contains_compatible(data: *const u8, len: usize, wanted: &[u8]) -> bool {
    if wanted.is_empty() || len < wanted.len() {
        return false;
    }

    let mut i = 0usize;
    while i + wanted.len() <= len {
        let mut same = true;
        for (j, &b) in wanted.iter().enumerate() {
            if *data.add(i + j) != b {
                same = false;
                break;
            }
        }

        if same {
            let left_ok = i == 0 || *data.add(i - 1) == 0;
            let right = i + wanted.len();
            let right_ok = right == len || *data.add(right) == 0;
            if left_ok && right_ok {
                return true;
            }
        }

        i += 1;
    }

    false
}

unsafe fn print_hex(label: &str, value: u64) {
    crate::uart_write(label);
    let mut buf = [0u8; 18];
    buf[0] = b'0';
    buf[1] = b'x';

    for i in 0..16 {
        let nib = ((value >> ((15 - i) * 4)) & 0xf) as u8;
        buf[2 + i] = if nib < 10 {
            b'0' + nib
        } else {
            b'a' + nib - 10
        };
    }

    if let Ok(s) = core::str::from_utf8(&buf) {
        crate::uart_write(s);
    }
    crate::uart_write("\n");
}

/// Discover all virtio,mmio transports described by QEMU's DTB.
///
/// Returns only addresses explicitly described by the platform. This avoids
/// probing made-up MMIO addresses and taking synchronous data aborts.
pub unsafe fn virtio_mmio_regions() -> ([MmioRegion; MAX_VIRTIO_MMIO], usize) {
    let base = DTB_ADDR as *const u8;
    let mut out = [MmioRegion::EMPTY; MAX_VIRTIO_MMIO];
    let mut out_len = 0usize;

    if be32(base, 0) != FDT_MAGIC {
        crate::uart_write("dtb: invalid magic at 0x40000000\n");
        return (out, 0);
    }

    let total_size = be32(base, 4) as usize;
    let off_struct = be32(base, 8) as usize;
    let off_strings = be32(base, 12) as usize;
    let size_strings = be32(base, 32) as usize;
    let size_struct = be32(base, 36) as usize;

    if total_size < 40
        || total_size > 1024 * 1024
        || off_struct >= total_size
        || off_strings >= total_size
        || off_struct + size_struct > total_size
        || off_strings + size_strings > total_size
    {
        crate::uart_write("dtb: malformed header\n");
        return (out, 0);
    }

    crate::uart_write("dtb: valid QEMU device tree\n");

    let struct_base = base.add(off_struct);
    let strings_base = base.add(off_strings);
    let mut pos = 0usize;
    let mut depth = 0usize;
    let mut stack = [NodeState::EMPTY; 32];

    let mut root_addr_cells = 2u32;
    let mut root_size_cells = 2u32;

    while pos + 4 <= size_struct {
        let token = be32(struct_base, pos);
        pos += 4;

        match token {
            FDT_BEGIN_NODE => {
                if depth >= stack.len() {
                    crate::uart_write("dtb: node nesting too deep\n");
                    return (out, out_len);
                }

                stack[depth] = NodeState::EMPTY;
                depth += 1;

                while pos < size_struct && *struct_base.add(pos) != 0 {
                    pos += 1;
                }
                if pos >= size_struct {
                    break;
                }
                pos = align4(pos + 1);
            }

            FDT_END_NODE => {
                if depth == 0 {
                    break;
                }

                let node = stack[depth - 1];
                if node.virtio_mmio && node.have_reg && out_len < out.len() {
                    out[out_len] = MmioRegion {
                        base: node.reg_base,
                        size: node.reg_size,
                    };
                    print_hex("dtb: virtio-mmio @ ", node.reg_base as u64);
                    out_len += 1;
                }

                depth -= 1;
            }

            FDT_PROP => {
                if pos + 8 > size_struct || depth == 0 {
                    break;
                }

                let len = be32(struct_base, pos) as usize;
                let name_off = be32(struct_base, pos + 4) as usize;
                pos += 8;

                if pos + len > size_struct || name_off >= size_strings {
                    break;
                }

                let data = struct_base.add(pos);
                let name = strings_base.add(name_off);
                let name_max = size_strings - name_off;

                if depth == 1 && cstr_eq(name, name_max, b"#address-cells") && len >= 4 {
                    root_addr_cells = be32(data, 0).clamp(1, 2);
                } else if depth == 1 && cstr_eq(name, name_max, b"#size-cells") && len >= 4 {
                    root_size_cells = be32(data, 0).clamp(1, 2);
                } else if cstr_eq(name, name_max, b"compatible") {
                    if contains_compatible(data, len, b"virtio,mmio") {
                        stack[depth - 1].virtio_mmio = true;
                    }
                } else if cstr_eq(name, name_max, b"reg") {
                    let addr_bytes = root_addr_cells as usize * 4;
                    let size_bytes = root_size_cells as usize * 4;

                    if len >= addr_bytes + size_bytes {
                        let reg_base = be64_cells(data, root_addr_cells) as usize;
                        let reg_size =
                            be64_cells(data.add(addr_bytes), root_size_cells) as usize;

                        stack[depth - 1].reg_base = reg_base;
                        stack[depth - 1].reg_size = reg_size;
                        stack[depth - 1].have_reg = true;
                    }
                }

                pos = align4(pos + len);
            }

            FDT_NOP => {}

            FDT_END => break,

            _ => {
                crate::uart_write("dtb: unknown structure token\n");
                break;
            }
        }
    }

    if out_len == 0 {
        crate::uart_write("dtb: no virtio-mmio transports found\n");
    }

    (out, out_len)
}
