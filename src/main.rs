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
mod offline;
mod ui;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{Length, Size};
use cosmic::prelude::*;
use cosmic::widget;
use serde::{Deserialize, Serialize};
use std::process::Command as SysCommand;

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

// --------------------------------------------------------------- recipe ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub disk: String,
    pub filesystem: String,
    #[serde(default)]
    pub btrfs_subvolumes: bool,
    pub encryption: Encryption,
    /// Empty in live-ISO mode: bootc installs the running container.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub image: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_imgref: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bootloader: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compose_fs_backend: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flatpaks: Vec<String>,
    /// Embedded OCI stores for offline installs (spec §4B).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_image_stores: Vec<String>,
    #[serde(rename = "distroID")]
    pub distro_id: String,
    #[serde(default = "default_selinux")]
    pub selinux_disabled: bool,
    pub hostname: String,
}

fn default_selinux() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Encryption {
    #[serde(rename = "type")]
    pub enc_type: String,
    #[serde(default)]
    pub passphrase: String,
}

impl Default for Encryption {
    fn default() -> Self {
        Self {
            enc_type: "none".into(),
            passphrase: String::new(),
        }
    }
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            disk: String::new(),
            filesystem: "xfs".into(),
            btrfs_subvolumes: false,
            encryption: Encryption::default(),
            image: "ghcr.io/tuna-os/albacore:gnome".into(),
            target_imgref: String::new(),
            bootloader: String::new(),
            compose_fs_backend: false,
            flatpaks: Vec::new(),
            additional_image_stores: Vec::new(),
            distro_id: "tunaos".into(),
            selinux_disabled: true,
            hostname: "tunaos".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub name: String,
    pub size: String,
    pub model: String,
    pub transport: String,
}

pub const FILESYSTEMS: [&str; 2] = ["xfs", "btrfs"];
pub const ENCRYPTION_KINDS: [&str; 2] = ["none", "luks"];

// -------------------------------------------------------------- messages ----

#[derive(Debug, Clone)]
pub enum Message {
    NextPage,
    BackPage,
    SelectDisk(usize),
    DisksScanned(Result<Vec<DiskInfo>, String>),
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
            live = offline::live_iso_image();
            if live.is_some() {
                recipe.image = String::new();
            }
            recipe.additional_image_stores = offline::offline_stores();
        }

        if let Some(first) = disks.first() {
            recipe.disk = format!("/dev/{}", first.name);
        }

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
            capture: flags.capture,
        };

        let mut tasks = vec![app.set_window_title("TunaOS Installer".into())];
        if app.capture.is_some() {
            tasks.push(capture::begin());
        } else {
            tasks.push(Task::perform(Self::scan_disks(), |r| {
                cosmic::action::app(Message::DisksScanned(r))
            }));
        }
        app.set_header_title("TunaOS Installer".into());

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
                if let Some(kind) = ENCRYPTION_KINDS.get(idx) {
                    self.recipe.encryption.enc_type = (*kind).to_string();
                    if *kind == "none" {
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
