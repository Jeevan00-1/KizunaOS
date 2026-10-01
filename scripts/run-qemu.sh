#!/usr/bin/env bash
set -euo pipefail

kernel="${1:?cargo runner did not pass the kernel image}"

common=(
  -M virt
  -m 256M
  -serial mon:stdio
  -device ramfb
  -device virtio-keyboard-device
  -device virtio-tablet-device
)

case "$(uname -s)" in
  Darwin)
    exec qemu-system-aarch64       "${common[@]}"       -accel hvf       -cpu host       -display cocoa       -kernel "$kernel"
    ;;

  Linux)
    if [[ -r /dev/kvm ]]; then
      exec qemu-system-aarch64         "${common[@]}"         -accel kvm         -cpu host         -display gtk,zoom-to-fit=on         -kernel "$kernel"
    else
      exec qemu-system-aarch64         "${common[@]}"         -accel tcg         -cpu cortex-a72         -display gtk,zoom-to-fit=on         -kernel "$kernel"
    fi
    ;;

  *)
    echo "Unsupported host OS: $(uname -s)" >&2
    exit 1
    ;;
esac
