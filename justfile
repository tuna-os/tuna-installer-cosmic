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

# Regenerate flatpak/cargo-sources.json from Cargo.lock.
#
# MUST be re-run whenever Cargo.lock changes. flatpak-builder runs
# `cargo --offline build`, so any crate that is in the lockfile but not in
# this file fails the Flatpak build. Unlike `cargo vendor`, this generator
# also emits the *git* dependencies (libcosmic, winit, accesskit, cryoglyph,
# …) and the `[source."<url>"] replace-with` stanzas cargo needs to resolve
# them without network.
cargo-sources:
    #!/usr/bin/env bash
    set -euo pipefail
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    curl -sSfL -o "$tmp/flatpak-cargo-generator.py" \
      https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
    python3 -m pip install --quiet aiohttp toml tomlkit
    python3 "$tmp/flatpak-cargo-generator.py" Cargo.lock -o flatpak/cargo-sources.json
    # The generator still names the vendor config `config`; cargo deprecated
    # that name in favour of `config.toml` and warns about it on every build.
    sed -i 's/"dest-filename": "config"$/"dest-filename": "config.toml"/' flatpak/cargo-sources.json
    python3 -c 'import json,sys; json.load(open("flatpak/cargo-sources.json")); print("flatpak/cargo-sources.json regenerated")'

# Renovate automerges Cargo.lock bumps and nothing regenerates the vendored
# sources, so the two drift silently and "Build Flatpak OCI" on main is the
# only thing that notices — that is the 14-run outage. This is a structural
# comparison of the two committed files: no network, no clones, ~1 second.
#
# Fail if flatpak/cargo-sources.json has drifted from Cargo.lock.
check-cargo-sources:
    python3 .github/scripts/check-cargo-sources.py

# Verify the sync gate still rejects a desynced file. MUST detect the drift.
check-cargo-sources-selftest:
    python3 .github/scripts/check-cargo-sources.py --selftest
