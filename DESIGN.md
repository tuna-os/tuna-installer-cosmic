# TunaOS COSMIC installer design

This repository contains the libcosmic frontend for the
[fisherman](https://github.com/tuna-os/fisherman) bootc installation backend.
The frontend gathers and validates the user's choices, writes the fisherman
recipe, and presents the backend's progress and result.

## Flow

The installer has six screens:

1. **Welcome** introduces the detected TunaOS product.
2. **Select a disk** lists the devices returned by `lsblk -J` and requires a
   selection before continuing.
3. **Options** collects the hostname, filesystem, and encryption settings.
4. **Confirm** summarizes the choices and marks installation as a destructive
   action.
5. **Installing** writes a mode-0600 recipe under `XDG_RUNTIME_DIR`, starts
   fisherman, and streams its output.
6. **Finished** reports success or failure and prompts the user to restart.

The header bar shows the current step and overall progress. Navigation is
reversible through Confirm; starting installation is the boundary after which
the UI no longer offers Back or Continue.

## COSMIC integration

The app implements `cosmic::Application` and uses libcosmic widgets, spacing,
colors, and the user's active accent. Structural colors come from the COSMIC
theme rather than fixed RGB values so that light and dark themes remain
legible. The destructive action and status text use the corresponding COSMIC
semantic styles.

The layout is a centered, single-column wizard. Device choices use COSMIC list
items, configuration choices use settings sections, and the installation log
uses monospaced text. Copy is short, factual, and sentence case.

## Product and installation data

The displayed product name comes from the host's `PRETTY_NAME` in
`/etc/os-release` (or `/run/host/etc/os-release` in the Flatpak). The live ISO
path can leave the recipe's image field empty so fisherman installs the booted
bootc image without downloading it. Embedded OCI stores are passed through as
additional image stores.

## Capture mode

Setting `TUNA_CAPTURE_DIR` replaces disk and installation data with fixtures
and renders every screen for the
[GUI walkthrough](docs/gui-walkthrough.md). Capture mode refuses
`StartInstall`, so generating documentation cannot start fisherman or modify a
disk. The harness reads back the frame presented by wgpu and rejects blank or
nearly blank renders.

The CI capture runs under Xvfb with Mesa lavapipe. See the walkthrough and the
`capture` recipe in `justfile` for local prerequisites and commands.

## Accessibility and quality

Controls use explicit labels and disabled states to prevent incomplete input
from advancing. Theme-derived semantic colors preserve contrast across COSMIC
themes. Changes to the wizard should be checked with keyboard navigation and
by reviewing all generated screens in both the ordinary and variant-branded
capture artifacts.
