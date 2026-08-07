# Rebuild the walkthrough screenshots exactly the way CI does.
#
# Needs: mesa-vulkan-drivers (lavapipe), xvfb, imagemagick.
capture:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release
    for f in /usr/share/vulkan/icd.d/lvp_icd*.json; do
      [ -e "$f" ] && export VK_ICD_FILENAMES="$f" && break
    done
    export XDG_RUNTIME_DIR="$(mktemp -d)"; chmod 700 "$XDG_RUNTIME_DIR"
    export TUNA_CAPTURE_DIR=docs/screenshots
    mkdir -p docs/screenshots
    xvfb-run -a -s "-screen 0 1400x1000x24" ./target/release/tuna-installer-cosmic
    convert -delay 240 -loop 0 $(ls docs/screenshots/[0-9][0-9]-*.png | sort) docs/screenshots/walkthrough.gif

run:
    cargo run --release

check:
    cargo build

# Verify the pixel gate still rejects a blank render. MUST fail (exit 1).
capture-selftest:
    #!/usr/bin/env bash
    set -uo pipefail
    cargo build --release
    for f in /usr/share/vulkan/icd.d/lvp_icd*.json; do
      [ -e "$f" ] && export VK_ICD_FILENAMES="$f" && break
    done
    export XDG_RUNTIME_DIR="$(mktemp -d)"; chmod 700 "$XDG_RUNTIME_DIR"
    export TUNA_BLANK_SELFTEST=1 TUNA_CAPTURE_DIR="$(mktemp -d)"
    xvfb-run -a -s "-screen 0 1400x1000x24" ./target/release/tuna-installer-cosmic
    if [ $? -eq 0 ]; then echo "SELFTEST BROKEN: gate accepted a blank render" >&2; exit 1; fi
    echo "ok: gate rejected the blank render"
