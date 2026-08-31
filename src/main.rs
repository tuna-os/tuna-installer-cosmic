//! TunaOS installer — a real COSMIC application.
//!
//! This was previously plain `iced` with `.theme(|_| Theme::Dark)` hardcoded.
//! That is why it never looked like a COSMIC app: `libcosmic` was not a
//! dependency at all, so none of the COSMIC design system was linked in. It
//! also did not compile — the source was written against the iced 0.13 API
//! while `Cargo.toml` asked for 0.14, and nothing in CI ever built it.
//!
//! It is now a `cosmic::Application`: COSMIC header bar, `cosmic-theme`
//! palette and spacing, and the cosmic widget set.

mod capture;
mod model;
mod readiness;
mod offline;
mod product;
mod ui;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{Length, Size};
use cosmic::prelude::*;
use cosmic::widget;
use std::process::Command as SysCommand;

pub use model::{DiskInfo, Recipe, FILESYSTEMS};

pub const APP_ID: &str = "org.tunaos.InstallerCosmic";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let flags = Flags {
        capture: capture::Capture::from_env(),
    };

    let mut settings = Settings::default()
        // COSMIC ships its own icon theme, but the installer also runs on live
        // media that may only have Adwaita. freedesktop-icons falls back
        // through the theme's inherit chain, so naming Adwaita here means the
        // icons resolve in both places instead of silently drawing nothing —
        // which is exactly what the first screenshot run caught.
        .default_icon_theme("Adwaita")
        .size(Size::new(1000.0, 700.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(600.0)
                .min_height(480.0),
        );

    // Deterministic captures: the system preference is whatever the CI runner
    // happens to have, which would make the screenshots flap between light and
    // dark. Pin the theme when capturing, follow the user otherwise.
    if flags.capture.is_some() {
        settings = settings.theme(cosmic::theme::Theme::dark());
    }

    cosmic::app::run::<TunaInstaller>(settings, flags)?;
    Ok(())
}

pub struct Flags {
    pub capture: Option<capture::Capture>,
}

// ---------------------------------------------------------------- pages ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Welcome,
    DiskSelect,
    Options,
    Confirm,
    Installing,
    Done,
}

impl Page {
    /// Wizard order. Also the capture order, so a new page cannot be added
    /// without appearing in the walkthrough.
    pub const ORDER: [Page; 6] = [
        Page::Welcome,
        Page::DiskSelect,
        Page::Options,
        Page::Confirm,
        Page::Installing,
        Page::Done,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Page::Welcome => "welcome",
            Page::DiskSelect => "disk",
            Page::Options => "options",
            Page::Confirm => "confirm",
            Page::Installing => "installing",
            Page::Done => "done",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Page::Welcome => "Welcome",
            Page::DiskSelect => "Select a disk",
            Page::Options => "Options",
            Page::Confirm => "Confirm",
            Page::Installing => "Installing",
            Page::Done => "Finished",
        }
    }

    fn index(self) -> usize {
        Page::ORDER.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// One entry in the encryption picker.
///
/// `id` is what actually lands in `recipe.encryption.type` and it MUST be one
/// of the four values `fisherman` accepts (`fisherman/internal/recipe/recipe.go`
/// `Validate()`): "none", "luks-passphrase", "tpm2-luks",
/// "tpm2-luks-passphrase". Anything else — the previous `"luks"` here — fails
/// recipe validation and the install never starts, which is a worse failure
/// mode than never offering encryption at all: the user thinks they chose it.
#[derive(Debug, Clone, Copy)]
pub struct EncryptionChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Only offered when a TPM2 chip is present (tunaOS#734 / tuna-installer-xfce
    /// `ENCRYPTION_CHOICES`, which this mirrors value-for-value so a recipe
    /// produced by either frontend means the same thing).
    pub tpm: bool,
}

// `static`, not `const`: `available_encryption_choices` hands out
// `&'static EncryptionChoice`s borrowed straight from this table. A `const`
// has no fixed address (each use site may get its own inlined copy), so
// relying on it for a genuinely `'static` borrow depends on rvalue-static-
// promotion kicking in. `static` sidesteps the question — it has exactly one
// address for the life of the program, so the borrow is `'static` outright.
pub static ENCRYPTION_CHOICES: [EncryptionChoice; 4] = [
    EncryptionChoice {
        id: "none",
        label: "No encryption",
        description: "Anyone with the disk can read your files.",
        tpm: false,
    },
    EncryptionChoice {
        id: "luks-passphrase",
        label: "Passphrase",
        description: "You'll type it at every boot.",
        tpm: false,
    },
    EncryptionChoice {
        id: "tpm2-luks",
        label: "TPM",
        description: "Unlocks automatically on this hardware.",
        tpm: true,
    },
    EncryptionChoice {
        id: "tpm2-luks-passphrase",
        label: "TPM + passphrase",
        description: "Automatic unlock, passphrase as fallback.",
        tpm: true,
    },
];

/// The choices actually selectable right now. TPM-gated entries are dropped
/// entirely rather than shown-disabled when there is no TPM — same call
/// `tuna-installer-xfce` makes (`SetupPage.__init__`: `if value.startswith("tpm2")
/// and not self.has_tpm: continue`), and for the same reason: a dropdown entry
/// that silently produces an unenrollable recipe is worse than one that isn't
/// offered.
pub fn available_encryption_choices(has_tpm: bool) -> Vec<&'static EncryptionChoice> {
    ENCRYPTION_CHOICES
        .iter()
        .filter(|c| !c.tpm || has_tpm)
        .collect()
}

// -------------------------------------------------------------- messages ----

#[derive(Debug, Clone)]
pub enum Message {
    NextPage,
    BackPage,
    SelectDisk(usize),
    DisksScanned(Result<Vec<DiskInfo>, String>),
    /// Resolved asynchronously so init() never blocks on a host command.
    LiveImageResolved(Option<String>),
    HostnameChanged(String),
    FilesystemChanged(usize),
    EncryptionChanged(usize),
    PassphraseChanged(String),
    TogglePassphraseVisible,
    StartInstall,
    InstallFinished(Result<i32, String>),
    Quit,
    /// Capture harness only — never reachable from the UI.
    Capture(capture::Message),
}

pub struct TunaInstaller {
    core: Core,
    page: Page,
    live_image: Option<String>,
    recipe: Recipe,
    disks: Vec<DiskInfo>,
    selected_disk: Option<usize>,
    install_log: String,
    install_ok: bool,
    installing: bool,
    passphrase_hidden: bool,
    /// `/sys/class/tpm/tpm0` existence, checked once at startup — same probe
    /// `tuna-installer-xfce` uses. Gates the two `tpm2-*` encryption choices.
    has_tpm: bool,
    /// `Some` only in capture mode. Its presence is also the hard interlock
    /// that stops the capture harness ever running a real install.
    capture: Option<capture::Capture>,
}

impl TunaInstaller {
    pub fn page(&self) -> Page {
        self.page
    }
    pub fn recipe(&self) -> &Recipe {
        &self.recipe
    }
    pub fn disks(&self) -> &[DiskInfo] {
        &self.disks
    }
    pub fn selected_disk(&self) -> Option<usize> {
        self.selected_disk
    }
    pub fn live_image(&self) -> Option<&str> {
        self.live_image.as_deref()
    }
    pub fn install_log(&self) -> &str {
        &self.install_log
    }
    pub fn install_ok(&self) -> bool {
        self.install_ok
    }
    pub fn installing(&self) -> bool {
        self.installing
    }
    pub fn passphrase_hidden(&self) -> bool {
        self.passphrase_hidden
    }
    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }
    pub fn has_tpm(&self) -> bool {
        self.has_tpm
    }

    /// Whether the Options page is allowed to advance. The dropdown can only
    /// ever hold a valid `enc_type` (see `Message::EncryptionChanged`), so the
    /// one thing left to check is the passphrase fisherman's own `Validate()`
    /// requires for "luks-passphrase" and "tpm2-luks-passphrase": both contain
    /// the substring "passphrase", matching `tuna-installer-xfce`'s
    /// `"passphrase" in enc_type()` check. Without this gate, a user who left
    /// the field blank would sail through Confirm and only discover the
    /// problem when fisherman rejects the recipe on the Installing page.
    pub fn encryption_ok(&self) -> bool {
        if self.recipe.encryption.enc_type.contains("passphrase") {
            !self.recipe.encryption.passphrase.is_empty()
        } else {
            true
        }
    }

    fn advance(&mut self) -> Option<Page> {
        let next = match self.page {
            Page::Welcome => Page::DiskSelect,
            Page::DiskSelect => Page::Options,
            Page::Options => Page::Confirm,
            Page::Confirm | Page::Installing | Page::Done => return None,
        };
        self.page = next;
        Some(next)
    }

    fn retreat(&mut self) {
        self.page = match self.page {
            Page::Confirm => Page::Options,
            Page::Options => Page::DiskSelect,
            Page::DiskSelect => Page::Welcome,
            other => other,
        };
    }
}

impl cosmic::Application for TunaInstaller {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Flags) -> (Self, Task<Message>) {
        let capturing = flags.capture.is_some();

        let mut recipe = Recipe::default();
        let mut disks = Vec::new();
        let live;
        let mut init_tasks: Vec<Task<Message>> = Vec::new();

        if capturing {
            // Fixtures. In capture mode nothing shells out: no `bootc status`,
            // no `lsblk`, no `podman`. The machine in the screenshots does not
            // exist, so the output is deterministic and CI never sees a disk.
            disks = capture::fixture_disks();
            live = Some(capture::FIXTURE_LIVE_IMAGE.to_string());
            recipe.image = String::new();
        } else {
            // Offline install support (spec §4): live-ISO mode installs the
            // running container (empty image); embedded stores are always
            // passed.
            //
            // `live_iso_image` shells out to the host via flatpak-spawn and
            // MUST NOT block init(). Iced creates the window *after* init
            // returns, so a hung host command would leave the process alive
            // with no window — exactly the failure mode tunaOS#678 caught.
            // Defer it to an async task and start with `live_image = None`;
            // the Confirm page handles `None` gracefully (it just shows the
            // default image ref).
            init_tasks.push(Task::perform(
                tokio::task::spawn_blocking(offline::live_iso_image),
                |r| cosmic::action::app(Message::LiveImageResolved(r.unwrap_or(None))),
            ));
            recipe.additional_image_stores = offline::offline_stores();
            live = None;
        }

        if let Some(first) = disks.first() {
            recipe.disk = format!("/dev/{}", first.name);
        }

        // Same probe as tuna-installer-xfce (`os.path.exists("/sys/class/tpm/tpm0")`).
        // A read-only sysfs check, not a shell-out, so it runs unconditionally —
        // including under capture: the Xvfb CI runner has no TPM, so this comes
        // back false there and the tpm2-* choices simply don't appear in the
        // captured "options" screenshot, same as they wouldn't on real hardware
        // without a chip.
        let has_tpm = std::path::Path::new("/sys/class/tpm/tpm0").exists();

        let mut app = Self {
            core,
            page: Page::Welcome,
            live_image: live,
            recipe,
            selected_disk: (!disks.is_empty()).then_some(0),
            disks,
            install_log: String::new(),
            install_ok: false,
            installing: false,
            passphrase_hidden: true,
            has_tpm,
            capture: flags.capture,
        };

        // The variant name from /etc/os-release ("Skipjack"), not a hardcoded
        // "TunaOS" — see `product`.
        let window_title = format!("{} Installer", product::name());
        let mut tasks = vec![app.set_window_title(window_title.clone())];
        tasks.append(&mut init_tasks);
        if app.capture.is_some() {
            tasks.push(capture::begin());
        } else {
            tasks.push(Task::perform(Self::scan_disks(), |r| {
                cosmic::action::app(Message::DisksScanned(r))
            }));
        }
        app.set_header_title(window_title);

        (app, Task::batch(tasks))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        // The step indicator lives in the COSMIC header bar rather than being
        // drawn by hand in the page body, which is what makes this read as a
        // COSMIC app instead of a generic iced window.
        let spacing = cosmic::theme::active().cosmic().spacing;
        vec![widget::text::caption(format!(
            "Step {} of {} · {}",
            self.page.index() + 1,
            Page::ORDER.len(),
            self.page.title()
        ))
        .apply(widget::container)
        .padding([0, spacing.space_xs])
        .into()]
    }

    fn header_end(&self) -> Vec<Element<'_, Message>> {
        vec![widget::progress_bar::determinate_linear(
            (self.page.index() as f32 + 1.0) / Page::ORDER.len() as f32,
        )
        .width(Length::Fixed(120.0))
        .into()]
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NextPage => {
                if let Some(Page::DiskSelect) = self.advance() {
                    if !self.capturing() {
                        return Task::perform(Self::scan_disks(), |r| {
                            cosmic::action::app(Message::DisksScanned(r))
                        });
                    }
                }
                Task::none()
            }
            Message::BackPage => {
                self.retreat();
                Task::none()
            }
            Message::SelectDisk(idx) => {
                if let Some(disk) = self.disks.get(idx) {
                    self.selected_disk = Some(idx);
                    self.recipe.disk = format!("/dev/{}", disk.name);
                }
                Task::none()
            }
            Message::DisksScanned(Ok(disks)) => {
                self.disks = disks;
                if !self.disks.is_empty() && self.selected_disk.is_none() {
                    self.selected_disk = Some(0);
                    self.recipe.disk = format!("/dev/{}", self.disks[0].name);
                }
                Task::none()
            }
            Message::DisksScanned(Err(err)) => {
                self.install_log
                    .push_str(&format!("Disk scan error: {err}\n"));
                Task::none()
            }
            Message::LiveImageResolved(live) => {
                self.live_image = live;
                if self.live_image.is_some() {
                    self.recipe.image = String::new();
                }
                Task::none()
            }
            Message::HostnameChanged(h) => {
                self.recipe.hostname = h;
                Task::none()
            }
            Message::FilesystemChanged(idx) => {
                if let Some(fs) = FILESYSTEMS.get(idx) {
                    self.recipe.filesystem = (*fs).to_string();
                    self.recipe.btrfs_subvolumes = *fs == "btrfs";
                }
                Task::none()
            }
            Message::EncryptionChanged(idx) => {
                // Recompute the same has_tpm-filtered list ui.rs built the
                // dropdown from, so `idx` (a position in THAT list) resolves
                // to the same choice the user actually saw and clicked.
                let choices = available_encryption_choices(self.has_tpm);
                if let Some(choice) = choices.get(idx) {
                    self.recipe.encryption.enc_type = choice.id.to_string();
                    // Only "luks-passphrase" and "tpm2-luks-passphrase" carry a
                    // passphrase; clear it for "none" and bare "tpm2-luks" so a
                    // stale value from a previous choice can't linger into the
                    // recipe (fisherman ignores it, but Confirm would still
                    // display it — see `.contains("passphrase")` above).
                    if !choice.id.contains("passphrase") {
                        self.recipe.encryption.passphrase.clear();
                    }
                }
                Task::none()
            }
            Message::PassphraseChanged(p) => {
                self.recipe.encryption.passphrase = p;
                Task::none()
            }
            Message::TogglePassphraseVisible => {
                self.passphrase_hidden = !self.passphrase_hidden;
                Task::none()
            }
            Message::StartInstall => {
                // SAFETY INTERLOCK. Driving the wizard to the progress page
                // must never partition the CI runner's disk. A sibling repo
                // (tuna-installer-xfce) called start_install() from the
                // page-enter hook, so a naive capture would have done exactly
                // that. Here the capture harness sets `page` directly and
                // never emits StartInstall — and if it ever did, this refuses.
                if self.capturing() {
                    tracing::error!("StartInstall ignored: capture mode");
                    return Task::none();
                }
                self.page = Page::Installing;
                self.installing = true;
                let recipe = self.recipe.clone();
                Task::perform(Self::run_fisherman(recipe), |r| {
                    cosmic::action::app(Message::InstallFinished(r))
                })
            }
            Message::InstallFinished(result) => {
                self.page = Page::Done;
                self.installing = false;
                match result {
                    Ok(code) => {
                        self.install_ok = code == 0;
                        self.install_log
                            .push_str(&format!("\n=== fisherman exited with code {code} ===\n"));
                    }
                    Err(e) => {
                        self.install_ok = false;
                        self.install_log.push_str(&format!("\n=== Error: {e} ===\n"));
                    }
                }
                Task::none()
            }
            Message::Quit => {
                std::process::exit(i32::from(!self.install_ok));
            }
            Message::Capture(msg) => capture::update(self, msg),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        // First frame = the strongest honest "the UI came up" signal libcosmic
        // offers. See readiness.rs: this frontend is the one that proved
        // `flatpak ps` insufficient, by running with no window ever appearing
        // while the smoke check stayed green. Cheap after the first call.
        readiness::stamp_first_frame(APP_ID, self.page.slug());
        ui::view(self)
    }
}

// -------------------------------------------------------- async helpers ----

impl TunaInstaller {
    async fn scan_disks() -> Result<Vec<DiskInfo>, String> {
        let output = tokio::task::spawn_blocking(|| {
            SysCommand::new("lsblk")
                .args(["-J", "-o", "NAME,SIZE,TYPE,MODEL,TRAN"])
                .output()
                .map_err(|e| format!("Failed to run lsblk: {e}"))
        })
        .await
        .map_err(|e| format!("Task join error: {e}"))??;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }

        let val: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Failed to parse lsblk JSON: {e}"))?;

        let mut disks = Vec::new();
        if let Some(devices) = val.get("blockdevices").and_then(|d| d.as_array()) {
            for dev in devices {
                if dev.get("type").and_then(|t| t.as_str()) == Some("disk") {
                    let field = |k: &str| {
                        dev.get(k)
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    };
                    disks.push(DiskInfo {
                        name: field("name"),
                        size: field("size"),
                        model: field("model"),
                        transport: field("tran"),
                    });
                }
            }
        }
        Ok(disks)
    }

    async fn run_fisherman(recipe: Recipe) -> Result<i32, String> {
        let json = serde_json::to_string_pretty(&recipe).map_err(|e| e.to_string())?;
        // 0600 under XDG_RUNTIME_DIR — the recipe may hold a passphrase.
        let path = offline::write_recipe(&json).map_err(|e| e.to_string())?;

        let output = tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                // pkexec /app/bin/fisherman in Flatpak, sudo otherwise.
                let cmd = offline::fisherman_command();
                SysCommand::new(&cmd[0])
                    .args(&cmd[1..])
                    .arg(&path)
                    .output()
                    .map_err(|e| format!("Failed to run fisherman: {e}"))
            }
        })
        .await
        .map_err(|e| format!("Task join error: {e}"))??;

        let _ = std::fs::remove_file(&path);
        Ok(output.status.code().unwrap_or(-1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_default_values_and_json_serialization() {
        let recipe = Recipe::default();
        assert_eq!(recipe.filesystem, "xfs");
        assert_eq!(recipe.encryption.enc_type, "none");
        assert_eq!(recipe.distro_id, "tunaos");
        assert_eq!(recipe.hostname, "tunaos");
        assert!(recipe.selinux_disabled);

        let json_str = serde_json::to_string(&recipe).unwrap();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(json["filesystem"], "xfs");
        assert_eq!(json["encryption"]["type"], "none");
        assert_eq!(json["distroID"], "tunaos");
        assert_eq!(json["hostname"], "tunaos");
        assert_eq!(json["selinuxDisabled"], true);
        assert!(json.get("image").is_some());
        assert!(json.get("targetImgref").is_none());
        assert!(json.get("bootloader").is_none());
    }

    #[test]
    fn encryption_choices_filtering() {
        let without_tpm = available_encryption_choices(false);
        assert_eq!(without_tpm.len(), 2);
        assert!(without_tpm.iter().all(|c| !c.tpm));
        assert_eq!(without_tpm[0].id, "none");
        assert_eq!(without_tpm[1].id, "luks-passphrase");

        let with_tpm = available_encryption_choices(true);
        assert_eq!(with_tpm.len(), 4);
        assert_eq!(with_tpm[2].id, "tpm2-luks");
        assert_eq!(with_tpm[3].id, "tpm2-luks-passphrase");
    }

    #[test]
    fn recipe_roundtrip_and_field_serialization() {
        let mut recipe = Recipe::default();
        recipe.disk = "/dev/nvme0n1".into();
        recipe.filesystem = "btrfs".into();
        recipe.btrfs_subvolumes = true;
        recipe.target_imgref = "ghcr.io/tuna-os/albacore:stable".into();
        recipe.bootloader = "systemd".into();
        recipe.compose_fs_backend = true;
        recipe.flatpaks = vec!["org.mozilla.firefox".into()];
        recipe.additional_image_stores = vec!["/run/media/oci".into()];
        recipe.encryption.enc_type = "luks-passphrase".into();
        recipe.encryption.passphrase = "secret123".into();

        let json_str = serde_json::to_string(&recipe).unwrap();
        let restored: Recipe = serde_json::from_str(&json_str).unwrap();

        assert_eq!(restored.disk, recipe.disk);
        assert_eq!(restored.filesystem, recipe.filesystem);
        assert_eq!(restored.btrfs_subvolumes, recipe.btrfs_subvolumes);
        assert_eq!(restored.target_imgref, recipe.target_imgref);
        assert_eq!(restored.bootloader, recipe.bootloader);
        assert_eq!(restored.compose_fs_backend, recipe.compose_fs_backend);
        assert_eq!(restored.flatpaks, recipe.flatpaks);
        assert_eq!(restored.additional_image_stores, recipe.additional_image_stores);
        assert_eq!(restored.encryption.enc_type, recipe.encryption.enc_type);
        assert_eq!(restored.encryption.passphrase, recipe.encryption.passphrase);
    }
}
