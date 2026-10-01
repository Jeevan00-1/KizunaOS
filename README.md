# KizunaOS

An experimental ARM64 operating system written in Rust.

KizunaOS is being built from bare metal upward on QEMU `virt`, with the goal of becoming a small but real graphical operating system rather than only a boot demo.

## Current development line

**v0.1.0 desktop alpha** — branch: `dev/v0.1.0-desktop-alpha`

The project currently has:

- [x] AArch64 bare-metal boot at EL1
- [x] PL011 serial output and input
- [x] AArch64 exception vector table
- [x] Full trap frame + `eret` recovery path
- [x] Interactive kernel monitor
- [x] PSCI reboot / poweroff
- [x] 1 MiB free-list heap allocator with chunk reuse
- [x] QEMU `ramfb` framebuffer through fw_cfg DMA
- [x] Software backbuffer + present path
- [x] Anti-aliased framebuffer text rendering
- [x] Graphical desktop-alpha shell
- [x] Graphical terminal mirroring the kernel monitor
- [x] VirtIO MMIO device discovery
- [x] VirtIO tablet input
- [x] VirtIO keyboard input
- [x] Software mouse cursor overlay
- [x] Non-blocking kernel event loop (polled)
- [ ] GIC + ARM generic timer interrupts
- [ ] Interrupt-driven input
- [ ] Preemptive scheduler
- [ ] MMU / page tables
- [ ] EL0 userspace
- [ ] SVC syscall ABI
- [ ] VFS + block storage
- [ ] Userspace terminal and applications

## Architecture

- CPU: AArch64
- Kernel language: Rust (`no_std`)
- Primary target: QEMU `virt`
- Display: QEMU `ramfb`, XRGB8888, 800x600
- Input: VirtIO keyboard + absolute tablet over MMIO
- Serial: PL011 UART
- Current privilege level: EL1

## Run

```bash
cargo run
```

The QEMU window is directly interactive in the current alpha: move the host pointer over it and type after focusing the window. The launch terminal remains available as the serial/debug console too.

Inside the Kizuna terminal:

```text
input
help
heap
regs
mem
```

## v0.1.x direction

The next large subsystem pass is:

1. GICv2 + ARM generic timer IRQs
2. convert polling into interrupt-driven events
3. scheduler + kernel tasks
4. MMU and address spaces
5. EL0 process launch
6. syscall ABI
7. VirtIO block storage + VFS
8. move the terminal out of the kernel and into userspace
