//! COSMIC-faithful tokens for the raw-Iced frontend.
//!
//! DESIGN.md: "Migrate from raw Iced to libcosmic widgets/theme when
//! practical; until then, mirror its metrics (radius 8, spacing 8/12/16/24,
//! cosmic dark/light palettes)." This module is that interim step.
//!
//! What it deliberately does NOT do: read the user's accent colour or
//! light/dark preference. That needs `cosmic-config`, which needs libcosmic —
//! see issue #7. The neutrals below approximate COSMIC dark so the window
//! stops reading as a stock Iced app next to cosmic-panel; they are not
//! COSMIC's palette and should be deleted, not tuned, when libcosmic lands.

use iced::border::Radius;
use iced::widget::button;
use iced::{Border, Color, Font, Theme};

// ── Brand tokens (DESIGN.md "Tokens") ───────────────────────────────────────
// The only two brand colours. Everything structural is neutral/accent.

/// `--sonar` — offline chip, install-progress bar fill.
pub const SONAR: Color = Color::from_rgb(
    0x2E as f32 / 255.0,
    0xC4 as f32 / 255.0,
    0xB6 as f32 / 255.0,
);

/// `--catch` — destructive confirm (Install button, erase warning).
pub const CATCH: Color = Color::from_rgb(
    0xF4 as f32 / 255.0,
    0xA2 as f32 / 255.0,
    0x59 as f32 / 255.0,
);

// ── Neutrals (approximate COSMIC dark; superseded by cosmic-theme) ──────────

const BG: Color = Color::from_rgb(0.10, 0.10, 0.11);
const SURFACE: Color = Color::from_rgb(0.16, 0.16, 0.17);
const TEXT: Color = Color::from_rgb(0.92, 0.92, 0.93);
/// Captions and de-emphasised metadata. Replaces the hardcoded 0.5 greys.
pub const TEXT_DIM: Color = Color::from_rgb(0.62, 0.62, 0.64);
const ACCENT: Color = Color::from_rgb(0.36, 0.56, 0.85);
const DANGER: Color = Color::from_rgb(0.85, 0.35, 0.35);
const SUCCESS: Color = Color::from_rgb(0.35, 0.75, 0.55);

// ── Metrics (DESIGN.md) ─────────────────────────────────────────────────────

/// Corner radius. COSMIC's cards and controls are 8 px, not Iced's default.
pub const RADIUS: f32 = 8.0;

/// Spacing scale — 8 / 12 / 16 / 24. Use these rather than ad-hoc numbers.
/// SPACE_XS and HEADING are unused today but are part of the DESIGN.md scale;
/// the card gallery (DESIGN.md "Signature element") needs both.
#[allow(dead_code)]
pub const SPACE_XS: u16 = 8;
pub const SPACE_S: u16 = 12;
pub const SPACE_M: u16 = 16;
pub const SPACE_L: u16 = 24;

/// Content column cap. DESIGN.md: "Single centered column, max 760 px".
pub const CONTENT_MAX_WIDTH: f32 = 760.0;

// ── Type scale (DESIGN.md "Type") ───────────────────────────────────────────
// title 24/600, heading 18/600, body 14/400, caption 12/400.

pub const TITLE: u16 = 24;
#[allow(dead_code)]
pub const HEADING: u16 = 18;
pub const BODY: u16 = 14;
pub const CAPTION: u16 = 12;

/// COSMIC's UI font. Falls back automatically if the runtime lacks it.
pub const UI_FONT: Font = Font::with_name("Fira Sans");

/// Device names, OCI refs, log output.
pub const MONO_FONT: Font = Font::with_name("Fira Mono");

/// The palette handed to `iced::application().theme(..)`.
pub fn theme() -> Theme {
    Theme::custom(
        "TunaOS COSMIC".to_string(),
        iced::theme::Palette {
            background: BG,
            text: TEXT,
            primary: ACCENT,
            success: SUCCESS,
            danger: DANGER,
        },
    )
}

/// Primary action (Continue / Get started). COSMIC radius, theme accent.
pub fn button_primary(theme: &Theme, status: button::Status) -> button::Style {
    let accent = theme.extended_palette().primary.strong.color;
    styled(
        match status {
            button::Status::Hovered => lighten(accent, 0.08),
            button::Status::Pressed => lighten(accent, -0.06),
            button::Status::Disabled => Color { a: 0.4, ..accent },
            button::Status::Active => accent,
        },
        Color::WHITE,
    )
}

/// Secondary action (Back / Close). Quiet surface, same geometry.
pub fn button_secondary(_theme: &Theme, status: button::Status) -> button::Style {
    styled(
        match status {
            button::Status::Hovered => lighten(SURFACE, 0.06),
            button::Status::Pressed => lighten(SURFACE, -0.04),
            button::Status::Disabled => Color { a: 0.4, ..SURFACE },
            button::Status::Active => SURFACE,
        },
        TEXT,
    )
}

/// Destructive confirm. DESIGN.md assigns `--catch` to the Install button,
/// because that is the click that erases the disk.
pub fn button_destructive(_theme: &Theme, status: button::Status) -> button::Style {
    styled(
        match status {
            button::Status::Hovered => lighten(CATCH, 0.08),
            button::Status::Pressed => lighten(CATCH, -0.06),
            button::Status::Disabled => Color { a: 0.4, ..CATCH },
            button::Status::Active => CATCH,
        },
        // --catch is a light amber; white text on it fails contrast.
        Color::from_rgb(0.12, 0.10, 0.08),
    )
}

fn styled(background: Color, text_color: Color) -> button::Style {
    button::Style {
        background: Some(background.into()),
        text_color,
        border: Border {
            radius: Radius::from(RADIUS),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Positive `amount` lightens, negative darkens. Kept trivial on purpose —
/// libcosmic supplies real state colours and this all goes away with it.
fn lighten(c: Color, amount: f32) -> Color {
    Color {
        r: (c.r + amount).clamp(0.0, 1.0),
        g: (c.g + amount).clamp(0.0, 1.0),
        b: (c.b + amount).clamp(0.0, 1.0),
        a: c.a,
    }
}
