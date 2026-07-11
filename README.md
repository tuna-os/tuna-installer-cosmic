# TunaOS COSMIC Installer — Iced/Rust frontend for fisherman

**COSMIC-style (Iced/Rust) installer** that drives the fisherman bootc install backend.

## Workflow

1. **Welcome** — brief intro
2. **Disk Selection** — `lsblk -J` lists available disks; user picks one
3. **Confirm** — summary of choices
4. **Install** — writes recipe JSON, runs `fisherman`, streams output
5. **Done** — success/failure

## Build

```bash
# Dependencies
rustup update
cargo build --release

# Run
cargo run --release
```

## Recipe

Produces the same JSON recipe as the Qt/KDE installer.

## License

GPL-3.0-only

## Flatpak

```bash
# One-time: generate offline cargo sources for flatpak-builder
pip install aiohttp toml && python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json

flatpak-builder --user --install --force-clean build flatpak/org.tunaos.InstallerCosmic.json
flatpak run org.tunaos.InstallerCosmic
```

`flatpak-cargo-generator.py` comes from
https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo

## Offline installs

On a TunaOS live ISO the installer detects the booted bootc image
(`bootc status`) and installs it without a download (empty `image` in the
recipe). Embedded OCI stores listed in `/etc/tuna-installer/offline-stores`
(or `$TUNA_OFFLINE_STORES`, default `/usr/share/tuna-installer/oci-store`)
are passed to fisherman as `additionalImageStores`.
