#!/usr/bin/env bash
set -euo pipefail

kernel="${1:?cargo runner did not pass the kernel image}"

common=(
  -M virt
  -m 256M
  -serial mon:stdio
  -device ramfb,id=ramfb0
  -device virtio-keyboard-device,id=kbd0
  -device virtio-tablet-device,id=tablet0
)

case "$(uname -s)" in
  Darwin)
    # Homebrew's QEMU is Cocoa-only on macOS. Recent macOS builds can show a
    # Cocoa QEMU window that receives mouse events while the terminal remains
    # the active/key application, so keyDown events never reach QEMU.
    #
    # Keep QEMU in the foreground (so serial stdin still works) and use a tiny
    # helper to raise/activate the Cocoa process after the window appears.
    (
      sleep 0.8
      osascript <<'APPLESCRIPT' >/dev/null 2>&1 || true
tell application "System Events"
    if exists process "qemu-system-aarch64" then
        tell process "qemu-system-aarch64"
            set frontmost to true
            try
                perform action "AXRaise" of window 1
            end try
        end tell
    end if
end tell
APPLESCRIPT
    ) &

    exec qemu-system-aarch64       "${common[@]}"       -name KizunaOS       -accel hvf       -cpu host       -display cocoa,zoom-to-fit=on,zoom-interpolation=on,full-grab=on       -kernel "$kernel"
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
