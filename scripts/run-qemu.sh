#!/usr/bin/env bash
set -euo pipefail

kernel="${1:?cargo runner did not pass the kernel image}"

common=(
  -M virt
  -m 256M
  -device ramfb,id=ramfb0
  -device virtio-keyboard-device,id=kbd0
  -device virtio-tablet-device,id=tablet0
)

case "$(uname -s)" in
  Darwin)
    # Launch QEMU through LaunchServices as a real .app. On some macOS builds,
    # a Cocoa program started directly from a terminal can receive mouse events
    # while never becoming the key application, so NSEventTypeKeyDown continues
    # going to the terminal. A proper app-bundle launch avoids that host bug.
    repo_root="$(cd "$(dirname "$0")/.." && pwd)"
    runtime="$repo_root/target/kizuna-qemu-runtime"
    app="$runtime/KizunaQEMU.app"
    socket="$runtime/serial.sock"
    pidfile="$runtime/qemu.pid"
    qemu_bin="$(command -v qemu-system-aarch64)"

    rm -rf "$runtime"
    mkdir -p "$app/Contents/MacOS"

    cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  <key>CFBundleExecutable</key><string>KizunaQEMU</string>
  <key>CFBundleIdentifier</key><string>os.kizuna.qemu</string>
  <key>CFBundleName</key><string>KizunaQEMU</string>
  <key>CFBundleDisplayName</key><string>KizunaOS</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>0.1</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

    ln -sf "$qemu_bin" "$app/Contents/MacOS/KizunaQEMU"

    open -n -W "$app" --args       "${common[@]}"       -name KizunaOS       -accel hvf       -cpu host       -display cocoa,zoom-to-fit=on,zoom-interpolation=on,full-grab=on       -monitor none       -chardev "socket,id=serial0,path=$socket,server=on,wait=off"       -serial chardev:serial0       -pidfile "$pidfile"       -kernel "$kernel" &
    open_pid=$!

    cleanup() {
      if [[ -f "$pidfile" ]]; then
        kill "$(cat "$pidfile")" 2>/dev/null || true
      fi
      kill "$open_pid" 2>/dev/null || true
      rm -f "$socket" "$pidfile"
    }
    trap cleanup EXIT INT TERM

    for _ in {1..100}; do
      [[ -S "$socket" ]] && break
      if ! kill -0 "$open_pid" 2>/dev/null; then
        wait "$open_pid"
        exit $?
      fi
      sleep 0.05
    done

    if [[ ! -S "$socket" ]]; then
      echo "KizunaOS: QEMU serial socket did not appear" >&2
      wait "$open_pid"
      exit 1
    fi

    echo "KizunaOS: QEMU launched as a real macOS app; serial fallback attached below."
    echo "KizunaOS: click the QEMU window and type there. Ghostty remains a debug console."
    nc -U "$socket" || true
    wait "$open_pid" || true
    ;;

  Linux)
    if [[ -r /dev/kvm ]]; then
      exec qemu-system-aarch64         "${common[@]}"         -serial mon:stdio         -accel kvm         -cpu host         -display gtk,zoom-to-fit=on         -kernel "$kernel"
    else
      exec qemu-system-aarch64         "${common[@]}"         -serial mon:stdio         -accel tcg         -cpu cortex-a72         -display gtk,zoom-to-fit=on         -kernel "$kernel"
    fi
    ;;

  *)
    echo "Unsupported host OS: $(uname -s)" >&2
    exit 1
    ;;
esac
