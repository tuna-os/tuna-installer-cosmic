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
