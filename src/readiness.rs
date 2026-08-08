//! Readiness stamp: a machine-readable record that the UI really came up.
//!
//! # Why this exists
//!
//! tunaOS's `installer-smoke.yml` proves the frontend is up with `flatpak ps`,
//! which answers "is the process alive". That is not the same question as "did
//! the user get a window", and **this frontend is the one that proved it**: the
//! COSMIC leg ran the installer process with no window ever appearing on
//! screen, and the check stayed green. The only thing that noticed was a human
//! looking at a screenshot.
//!
//! Inferring it from pixels is the other half of the same problem — it needs a
//! compositor that renders, and four of the five desktops need a DRM render
//! node that GitHub-hosted runners do not have. So the app says so itself, in a
//! file any runner can read over SSH with no GPU and no OCR.
//!
//! # What this stamp can honestly claim
//!
//! Less than the GTK frontends' stamp, and the `signal` field says so.
//!
//! `bootc-installer` and `tuna-installer-xfce` stamp on GTK's `map` signal —
//! the widget actually being mapped by the compositor. libcosmic is
//! iced-on-wgpu and has no equivalent: the closest honest hook is the first
//! call to `view()`, which means the runtime asked us to build a frame.
//!
//! That is strictly weaker. A `view()` call proves the event loop is running
//! and producing frames; it does not prove a surface was mapped and presented.
//! It is still far stronger than `flatpak ps`, which is satisfied by a process
//! that has done nothing at all.
//!
//! Rather than paper over the difference and let a reader treat the five
//! frontends' stamps as equivalent, the stamp records `signal=first-frame`
//! here and `signal=gtk-map` there, so the smoke test can weigh them
//! differently — or demand the stronger one where it is available.
//!
//! `capture.rs` already makes the related point about screenshots: a valid PNG
//! can come from a surface that never acquired a real adapter, which is why the
//! capture harness reads back the presented frame instead of trusting the file.
//! Same instinct, different question.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// `$XDG_RUNTIME_DIR` is per-user, tmpfs, and cleared between sessions, so a
/// stale stamp cannot survive a reboot and be read as a fresh success.
const STAMP_NAME: &str = "tuna-installer-ready";

static STAMPED: AtomicBool = AtomicBool::new(false);

/// Write the stamp the first time this is called; a no-op thereafter.
///
/// Called from `view()`, which runs on every frame, so the `AtomicBool` is what
/// keeps this from being a per-frame write syscall.
///
/// Best-effort by design. A frontend that cannot write its stamp must still
/// install: this is observability, and taking the installer down because a
/// tmpfs was read-only would be a far worse bug than the one it detects.
pub fn stamp_first_frame(app_id: &str, page: &str) {
    // `swap` rather than load-then-store: `view()` is not guaranteed to be
    // called from only one thread over the app's life, and a duplicated write
    // would be harmless but a torn one would not.
    if STAMPED.swap(true, Ordering::SeqCst) {
        return;
    }

    if let Err(err) = write_stamp(app_id, page) {
        // stderr, not a panic — see the best-effort note above. The live
        // session captures stderr into the journal, so this is recoverable
        // information rather than a silent loss.
        eprintln!("readiness: could not write stamp: {err}");
    }
}

fn write_stamp(app_id: &str, page: &str) -> std::io::Result<()> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "XDG_RUNTIME_DIR is not set",
        )
    })?;

    let mapped_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let body = format!(
        "app_id={app_id}\nwindow=TunaInstaller\nsignal=first-frame\nmapped_at={mapped_at:.3}\npage={page}\n"
    );

    // Write to a temp file and rename, so a reader over SSH never sees a
    // half-written stamp and concludes the wrong thing came up.
    let path = std::path::Path::new(&runtime_dir).join(STAMP_NAME);
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut fh = std::fs::File::create(&tmp)?;
        fh.write_all(body.as_bytes())?;
    }
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_body_carries_the_weaker_signal() {
        // The contract's whole point is that a reader can tell this stamp's
        // claim apart from the GTK frontends'. If this line ever changes to
        // gtk-map, the smoke test would start believing a frame callback
        // proves a mapped window.
        let dir = std::env::temp_dir().join(format!("tuna-ready-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: single-threaded test, and the var is read again immediately.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &dir) };

        write_stamp("org.tunaos.InstallerCosmic", "welcome").unwrap();
        let body = std::fs::read_to_string(dir.join(STAMP_NAME)).unwrap();

        assert!(body.contains("app_id=org.tunaos.InstallerCosmic"), "{body}");
        assert!(body.contains("signal=first-frame"), "{body}");
        assert!(body.contains("page=welcome"), "{body}");
        assert!(body.contains("mapped_at="), "{body}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_runtime_dir_is_an_error_not_a_panic() {
        // The installer must survive this. See the best-effort note.
        unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
        assert!(write_stamp("x", "y").is_err());
    }
}
