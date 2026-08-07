# tuna-installer-cosmic — Design

libcosmic/Rust frontend for COSMIC. Shared flow/contract: `../INSTALLER-FRONTENDS.md`.

## Direction

COSMIC has a strong, young design system (rounded 8 px cards, generous
padding, Fira Sans, accent-driven theming) — fighting it would read as broken,
not bold. So this frontend is the most system-faithful of the four, and its
identity comes from **content, not chrome**: the TunaOS species artwork
(albacore, yellowfin, skipjack, bonito SVGs shipped in fisherman's
`data/images/`) is the hero.

**Done:** the app is a `cosmic::Application` on **libcosmic**. Structural
colour and every gap come from `cosmic::theme::active().cosmic()` — its palette
and its `spacing` scale — rather than being mirrored by hand, so the installer
follows the user's COSMIC theme and accent automatically.

(It was previously raw Iced with a hardcoded `Theme::Dark` and no libcosmic
dependency, which is the whole reason it did not look like a COSMIC app.)

## Signature element: the species gallery

The Source step is a card gallery, one card per catalog leaf group:

- Species SVG at 96 px, name in Fira Sans 600, one-line description,
  the OCI ref in mono underneath at 60 % opacity.
- Offline-available cards get a filled `offline` chip (`--sonar` bg) and sort
  first; network-only cards show a subtle download glyph.
- Selection = COSMIC accent ring (respect the user's system accent color; do
  not hardcode ours here).
- On the live ISO, a full-width "Install TunaOS — this system, no download"
  card sits above the gallery, preselected.

The gallery is the only page with imagery. Every other page is quiet COSMIC
form layout.

## Tokens

Defer to COSMIC theme (`cosmic-theme` palette, user accent) for all
structural color. Brand appears in exactly two places:

| Token | Hex | Use |
|---|---|---|
| `--sonar` | `#2EC4B6` | Offline chip, install-progress bar fill |
| `--catch` | `#F4A259` | Destructive confirm (Install button, erase warning) |

## Type

- Fira Sans (COSMIC default) for all UI; sizes from the COSMIC scale
  (title 24/600, heading 18/600, body 14/400, caption 12/400).
- Fira Mono for device names, refs, log output.

## Layout

```
┌────────────────────────────────────────────────────────────┐
│  ● ● ● ● ○ ○ ○ ○                    Choose what to install │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ ⬤ Install TunaOS — this system      [no download]   │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐      │
│  │ (fish)   │ │ (fish)   │ │ (fish)   │ │ (fish)   │      │
│  │ Albacore │ │Yellowfin │ │ Skipjack │ │  Bonito  │      │
│  │ offline  │ │ offline  │ │    ↓     │ │    ↓     │      │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘      │
│                                                            │
│                              [ Back ]      [ Continue ]    │
└────────────────────────────────────────────────────────────┘
```

- Step indicator: COSMIC-style dot strip, top-left; label top-right.
- Single centered column, max 760 px; cards in a 4-up responsive grid
  (2-up below 900 px).
- Progress page: segmented bar (9 segments = fisherman steps) filled in
  `--sonar`, current step name as heading, log in a collapsible mono panel.

## Copy

COSMIC register: sentence case, short, factual. "Everything on this disk will
be erased." Buttons: Continue / Back / Install / Restart.

## Quality floor

Honor COSMIC light/dark and accent instantly (subscribe to theme changes).
Full keyboard traversal of the gallery grid. Species SVGs get accessible
names. Reduced-motion: no card hover lift, instant page transitions.
