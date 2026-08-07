//! Which product is this ISO?
//!
//! tunaOS is not one product: it builds per-variant images (Skipjack, Bonito,
//! Yellowfin, …) and its branding pipeline — `build_scripts/90-image-info.sh`
//! in tuna-os/tunaOS — bakes the variant's name into `PRETTY_NAME` in
//! `/etc/os-release`. The GNOME frontend reads that, which is why its welcome
//! screen says "Welcome to Skipjack". This fork hardcoded "TunaOS", so a
//! Skipjack ISO showed the wrong name on every screen.
//!
//! Resolved once, on first use, and cached: the value cannot change while the
//! installer is running, and `view()` runs every frame.

use std::sync::OnceLock;

/// Used when no `os-release` is readable or it carries no `PRETTY_NAME` —
/// a developer checkout, a CI runner, or any non-tunaOS host.
pub const FALLBACK: &str = "TunaOS";

/// Host first. The installer ships as a flatpak (`org.tunaos.InstallerCosmic`),
/// and inside that sandbox `/etc/os-release` describes the *runtime*, not the
/// live ISO the user is installing from. The host's real one is bind-mounted
/// at `/run/host/etc/os-release`, so it must win when both exist.
const OS_RELEASE_PATHS: [&str; 2] = ["/run/host/etc/os-release", "/etc/os-release"];

/// Overrides the resolved name. Exists for the screenshot harness: the CI
/// runner's `PRETTY_NAME` is "Ubuntu 24.04 LTS", and the committed docs
/// walkthrough should not read "Install Ubuntu". Not a user-facing knob.
const OVERRIDE_ENV: &str = "TUNA_PRODUCT_NAME";

/// The product name to show the user, e.g. "Skipjack".
pub fn name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(resolve).as_str()
}

fn resolve() -> String {
    if let Some(value) = std::env::var_os(OVERRIDE_ENV) {
        let value = value.to_string_lossy().trim().to_string();
        if !value.is_empty() {
            return value;
        }
    }
    for path in OS_RELEASE_PATHS {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Some(name) = pretty_name(&text) {
                return name;
            }
        }
    }
    FALLBACK.to_string()
}

/// Pull `PRETTY_NAME` out of os-release text. Returns `None` when the key is
/// absent or its value is empty, so the caller falls through to the next file.
fn pretty_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("PRETTY_NAME=") else {
            continue;
        };
        // os-release values are usually quoted; strip a matched pair.
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
            .trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::pretty_name;

    #[test]
    fn reads_and_unquotes() {
        assert_eq!(
            pretty_name("ID=tunaos\nPRETTY_NAME=\"Skipjack\"\n").as_deref(),
            Some("Skipjack")
        );
    }

    #[test]
    fn unquoted_and_single_quoted() {
        assert_eq!(pretty_name("PRETTY_NAME=Bonito\n").as_deref(), Some("Bonito"));
        assert_eq!(
            pretty_name("PRETTY_NAME='Yellowfin 42'\n").as_deref(),
            Some("Yellowfin 42")
        );
    }

    #[test]
    fn empty_or_missing_is_none() {
        assert_eq!(pretty_name("PRETTY_NAME=\"\"\n"), None);
        assert_eq!(pretty_name("PRETTY_NAME=\n"), None);
        assert_eq!(pretty_name("ID=fedora\n"), None);
    }

    #[test]
    fn ignores_other_keys_containing_the_name() {
        assert_eq!(
            pretty_name("CPE_NAME=x\nNAME=Tuna\nPRETTY_NAME=\"Skipjack 41\"\n").as_deref(),
            Some("Skipjack 41")
        );
    }
}
