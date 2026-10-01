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
- [x] Graphical terminal mirroring the live kernel monitor
- [ ] GIC + ARM generic timer interrupts
- [ ] Preemptive scheduler
- [ ] MMU / page tables
- [ ] EL0 userspace
- [ ] SVC syscall ABI
- [ ] VirtIO keyboard / pointer input
- [ ] VFS + block storage
- [ ] Userspace terminal and applications

## Architecture

- CPU: AArch64
- Kernel language: Rust (`no_std`)
- Primary target: QEMU `virt`
- Display: QEMU `ramfb`, XRGB8888, 800x600
- Serial: PL011 UART
- Current privilege level: EL1

## Run

```bash
cargo run
```

The QEMU window is the graphical desktop. The terminal that launched QEMU remains the serial/debug input channel for the current alpha.

## v0.1.x direction

The next large subsystem pass is:

1. GIC + timer IRQs
2. kernel event loop + scheduler
3. MMU and address spaces
4. EL0 process launch
5. syscalls
6. VirtIO input + storage
7. move the terminal out of the kernel and into userspace
