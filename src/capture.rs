//! Screenshot harness for `docs/screenshots` and the README walkthrough.
//!
//! Why this lives inside the binary rather than beside it: libcosmic is
//! iced-on-wgpu, so the only trustworthy source of "what the user sees" is the
//! frame wgpu actually presented. `iced::window::screenshot` reads that frame
//! back off the GPU. An external X11 grab would also work, but it can hand you
//! a valid PNG of a surface that never acquired a real adapter. Reading back
//! the presented frame cannot.
//!
//! Two properties matter and are enforced in `audit`:
//!
//!  * SAFETY. Driving the wizard to the install page must never start an
//!    install. In `tuna-installer-xfce` the page-enter hook called
//!    `start_install()`, so a naive capture would have partitioned the CI
//!    runner's disk. Here the harness assigns `page` directly and never emits
//!    `Message::StartInstall`, and `update()` refuses that message outright
//!    while `capture` is `Some`. Every hardware query is replaced by fixtures.
//!
//!  * HONESTY. A blank page is a perfectly good 65 KB PNG.
//!    `bootc-installer-asahi` shipped a completely empty settings screen behind
//!    a green tick because its check asserted only that the files existed and
//!    were non-empty. So we read the pixels back and assert properties that
//!    only hold when the UI really drew, with thresholds calibrated against
//!    measured output from real renders.

use cosmic::app::Task;
use cosmic::iced::window::Screenshot;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::{DiskInfo, Page, TunaInstaller};

pub const FIXTURE_LIVE_IMAGE: &str = "ghcr.io/tuna-os/albacore:gnome";

/// Rows occupied by the COSMIC header bar, excluded from the pixel audit.
/// Comfortably taller than the bar itself — the point is to be certain no
/// chrome leaks into the measurement, not to measure the bar precisely.
const HEADER_SKIP_PX: u32 = 56;

/// Frames to let the compositor and wgpu settle before grabbing. Grabbing too
/// early is the dangerous failure: it yields a valid PNG of a half-drawn
/// window, which looks like a screenshot and is not one.
const SETTLE_MS: u64 = 700;

pub fn fixture_disks() -> Vec<DiskInfo> {
    vec![
        DiskInfo {
            name: "nvme0n1".into(),
            size: "476.9G".into(),
            model: "SAMSUNG MZVL2512HCJQ".into(),
            transport: "nvme".into(),
        },
        DiskInfo {
            name: "sda".into(),
            size: "1.8T".into(),
            model: "WDC WD20SPZX-22UA7".into(),
            transport: "sata".into(),
        },
    ]
}

const FIXTURE_LOG: &str = "\
[1/9] Partitioning /dev/nvme0n1
  created EFI system partition (1.0 GiB, FAT32)
  created root partition (475.9 GiB)
[2/9] Formatting boot partitions
[3/9] Setting up encryption
  encryption: none
[4/9] Formatting root filesystem (xfs)
[5/9] Mounting target at /mnt
[6/9] Installing image ghcr.io/tuna-os/albacore:gnome
  pulling layers... 1.9 GiB
  applying ostree commit
[7/9] Installing bootloader
";

pub struct Capture {
    pub dir: PathBuf,
    pub index: usize,
    pub findings: Vec<Finding>,
}

impl Capture {
    /// `TUNA_CAPTURE_DIR` is the only way in. There is no UI affordance and no
    /// command-line flag that a user could trip over.
    pub fn from_env() -> Option<Self> {
        std::env::var_os("TUNA_CAPTURE_DIR").map(|dir| Self {
            dir: PathBuf::from(dir),
            index: 0,
            findings: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub colours: usize,
    /// Share of sampled pixels that are the single most common colour.
    pub flattest: f64,
    /// Share of sampled pixels that differ noticeably from that colour — text,
    /// icons, borders, controls. "Ink".
    pub ink: f64,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Show page N, then wait for it to settle.
    Show(usize),
    /// Read the presented frame back.
    Shoot,
    /// The frame arrived.
    Shot(Screenshot),
}

pub fn begin() -> Task<crate::Message> {
    cosmic::task::message(cosmic::action::app(crate::Message::Capture(Message::Show(
        0,
    ))))
}

fn settle_then(msg: Message) -> Task<crate::Message> {
    cosmic::task::future(async move {
        tokio::time::sleep(std::time::Duration::from_millis(SETTLE_MS)).await;
        cosmic::action::app(crate::Message::Capture(msg))
    })
}

pub fn update(app: &mut TunaInstaller, message: Message) -> Task<crate::Message> {
    match message {
        Message::Show(index) => {
            let Some(page) = Page::ORDER.get(index).copied() else {
                return finish(app);
            };

            // Assign the page directly. Going through the ordinary navigation
            // messages would emit StartInstall on the way to Installing.
            app.page = page;
            app.capture.as_mut().unwrap().index = index;

            match page {
                Page::Installing => {
                    app.installing = true;
                    app.install_log = FIXTURE_LOG.to_string();
                }
                Page::Done => {
                    app.installing = false;
                    app.install_ok = true;
                }
                _ => {}
            }

            settle_then(Message::Shoot)
        }
        Message::Shoot => {
            let Some(id) = app.core.main_window_id() else {
                eprintln!("capture: no main window");
                std::process::exit(2);
            };
            cosmic::iced::window::screenshot(id)
                .map(|shot| cosmic::action::app(crate::Message::Capture(Message::Shot(shot))))
        }
        Message::Shot(shot) => {
            let capture = app.capture.as_mut().unwrap();
            let index = capture.index;
            let page = Page::ORDER[index];
            let name = page.slug().to_string();

            let path = capture
                .dir
                .join(format!("{:02}-{}.png", index + 1, name));
            if let Err(e) = write_png(&path, &shot) {
                eprintln!("capture: failed writing {}: {e}", path.display());
                std::process::exit(2);
            }
            capture.findings.push(audit(&shot, &name));

            cosmic::task::message(cosmic::action::app(crate::Message::Capture(Message::Show(
                index + 1,
            ))))
        }
    }
}

fn write_png(path: &std::path::Path, shot: &Screenshot) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        shot.size.width,
        shot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .write_image_data(&shot.rgba)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// Measure what only holds when the UI really rendered.
fn audit(shot: &Screenshot, name: &str) -> Finding {
    let (w, h) = (shot.size.width, shot.size.height);
    let mut counts: std::collections::HashMap<(u8, u8, u8), u64> =
        std::collections::HashMap::new();
    let mut distinct: HashSet<(u8, u8, u8)> = HashSet::new();
    let mut samples: u64 = 0;

    // Skip the COSMIC header bar. It draws identically whether or not the page
    // below it rendered anything, so including it makes a blank page look
    // populated: measured, a totally empty content area still scored 1.57%
    // "ink" and 98.4% flat purely from chrome, which straddles the range real
    // pages occupy. Auditing only the content region separates the two
    // cleanly. Anything measured over the whole frame is not a usable signal.
    let top = HEADER_SKIP_PX.min(h);

    // Sample every 3rd pixel in each direction; enough resolution for the
    // statistics and ~9x cheaper than the full frame.
    for y in (top..h).step_by(3) {
        for x in (0..w).step_by(3) {
            let i = ((y as usize * w as usize) + x as usize) * 4;
            if i + 2 >= shot.rgba.len() {
                continue;
            }
            let px = (shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]);
            *counts.entry(px).or_insert(0) += 1;
            distinct.insert(px);
            samples += 1;
        }
    }

    let (bg, bg_count) = counts
        .iter()
        .max_by_key(|(_, c)| **c)
        .map(|(p, c)| (*p, *c))
        .unwrap_or(((0, 0, 0), 0));

    // "Ink" = pixels far enough from the dominant background to be something
    // the user can see. Chebyshev distance keeps it cheap and channel-aware.
    let mut ink: u64 = 0;
    for (px, count) in &counts {
        let d = (px.0 as i32 - bg.0 as i32)
            .abs()
            .max((px.1 as i32 - bg.1 as i32).abs())
            .max((px.2 as i32 - bg.2 as i32).abs());
        if d > 24 {
            ink += count;
        }
    }

    let samples = samples.max(1) as f64;
    Finding {
        name: name.to_string(),
        width: w,
        height: h,
        colours: distinct.len(),
        flattest: bg_count as f64 / samples,
        ink: ink as f64 / samples,
    }
}

// Thresholds calibrated against MEASURED output from real headless renders on
// this rig (Xvfb 1400x1000 + Mesa lavapipe, window 1000x700), auditing the
// content region only. Both ends were measured — the working end by capturing
// the real pages, the broken end by capturing the same six pages with `view()`
// forced to an empty container (`TUNA_BLANK_SELFTEST=1`, still wired up so the
// baseline can be re-derived rather than re-guessed):
//
//     page          colours   largest-flat     ink
//     welcome           180          97.3%    2.45%
//     disk              172          58.5%    1.15%
//     options           190          76.7%    2.32%
//     confirm           241          56.2%    2.92%
//     installing        215          54.6%    2.28%
//     done              223          98.2%    1.71%
//     ---- blank ----     8          99.4%    0.60%
//
// The gates below sit in the gap between those two worlds, placed nearer the
// broken end so that a genuinely sparse page (welcome and done are mostly
// empty space by design) does not fail. Every run re-prints the table, so if
// the UI drifts enough to move the numbers the log says so before the gate
// does.
//
// Two things this exercise actually taught, both of which would have been
// wrong if guessed:
//
//   * `largest-flat` is the WEAKEST signal here, not the strongest. Real pages
//     reach 98.2% and a blank one is 99.4% — barely a point apart, because a
//     sparse dark page genuinely is almost all one colour. It is kept only as
//     a backstop. Colour count is the sharp discriminator: 8 versus 172+.
//   * Measured over the WHOLE frame these numbers do not separate at all. The
//     COSMIC header bar draws either way, which alone scored a blank page
//     1.57% ink and 98.4% flat — inside the range real pages occupy. Hence
//     HEADER_SKIP_PX.
//
// The sibling repo's first attempt guessed 0.97 for largest-flat and then
// failed a page that had rendered perfectly. Guessing a threshold and reading
// the resulting failure as a defect is how you end up "fixing" working code.
const MIN_COLOURS: usize = 60;
const MAX_FLATTEST: f64 = 0.99;
const MIN_INK: f64 = 0.009;

fn finish(app: &mut TunaInstaller) -> Task<crate::Message> {
    let capture = app.capture.as_ref().unwrap();
    let mut failures: Vec<String> = Vec::new();

    println!(
        "\n  {:<12} {:>10}  {:>8}  {:>12}  {:>7}",
        "page", "size", "colours", "largest-flat", "ink"
    );
    for f in &capture.findings {
        println!(
            "  {:<12} {:>4}x{:<5} {:>8}  {:>11.1}%  {:>6.2}%",
            f.name,
            f.width,
            f.height,
            f.colours,
            f.flattest * 100.0,
            f.ink * 100.0
        );
        if f.colours < MIN_COLOURS {
            failures.push(format!(
                "{}: only {} distinct colours — did not render",
                f.name, f.colours
            ));
        }
        if f.flattest > MAX_FLATTEST {
            failures.push(format!(
                "{}: {:.1}% of the frame is one flat colour — blank page",
                f.name,
                f.flattest * 100.0
            ));
        }
        if f.ink < MIN_INK {
            failures.push(format!(
                "{}: {:.2}% ink — nothing legible was drawn",
                f.name,
                f.ink * 100.0
            ));
        }
    }

    if capture.findings.len() != Page::ORDER.len() {
        failures.push(format!(
            "captured {} of {} pages",
            capture.findings.len(),
            Page::ORDER.len()
        ));
    }

    if !failures.is_empty() {
        eprintln!();
        for msg in &failures {
            eprintln!("FAIL: {msg}");
        }
        std::process::exit(1);
    }

    println!(
        "\n  wrote {} screens to {}",
        capture.findings.len(),
        capture.dir.display()
    );
    std::process::exit(0);
}
