mod offline;
mod theme;

use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Alignment, Element, Length, Task};
use serde::{Deserialize, Serialize};
use std::process::Command as SysCommand;

fn main() -> iced::Result {
    tracing_subscriber::fmt::init();
    iced::application("TunaOS Installer", TunaInstaller::update, TunaInstaller::view)
        .window(iced::window::Settings {
            size: iced::Size::new(800.0, 600.0),
            ..Default::default()
        })
        .default_font(theme::UI_FONT)
        .theme(|_| theme::theme())
        .run_with(TunaInstaller::new)
}

#[derive(Debug, Clone)]
enum Page {
    Welcome,
    DiskSelect,
    Confirm,
    Installing,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recipe {
    disk: String,
    filesystem: String,
    #[serde(default)]
    btrfs_subvolumes: bool,
    encryption: Encryption,
    /// Empty in live-ISO mode: bootc installs the running container.
    #[serde(skip_serializing_if = "String::is_empty")]
    image: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    target_imgref: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    bootloader: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    compose_fs_backend: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    flatpaks: Vec<String>,
    /// Embedded OCI stores for offline installs (spec §4B).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    additional_image_stores: Vec<String>,
    #[serde(rename = "distroID")]
    distro_id: String,
    #[serde(default = "default_selinux")]
    selinux_disabled: bool,
    hostname: String,
}

fn default_selinux() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Encryption {
    #[serde(rename = "type")]
    enc_type: String,
    #[serde(default)]
    passphrase: String,
}

impl Default for Encryption {
    fn default() -> Self {
        Self { enc_type: "none".into(), passphrase: String::new() }
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
struct DiskInfo {
    name: String,
    size: String,
    model: String,
    transport: String,
}

#[derive(Debug, Clone)]
enum Message {
    NextPage,
    BackPage,
    SelectDisk(usize),
    DisksScanned(Result<Vec<DiskInfo>, String>),
    HostnameChanged(String),
    StartInstall,
    InstallOutput(String),
    InstallFinished(Result<i32, String>),
    Quit,
}

struct TunaInstaller {
    page: Page,
    live_image: Option<String>,
    recipe: Recipe,
    disks: Vec<DiskInfo>,
    selected_disk: Option<usize>,
    install_log: String,
    install_ok: bool,
}

impl TunaInstaller {
    fn new() -> (Self, Task<Message>) {
        let mut recipe = Recipe::default();
        // Offline install support (spec §4): live-ISO mode installs the
        // running container (empty image); embedded stores are always passed.
        let live = offline::live_iso_image();
        if live.is_some() {
            recipe.image = String::new();
        }
        recipe.additional_image_stores = offline::offline_stores();

        let app = Self {
            page: Page::Welcome,
            live_image: live,
            recipe,
            disks: Vec::new(),
            selected_disk: None,
            install_log: String::new(),
            install_ok: false,
        };
        let task = Task::perform(Self::scan_disks(), Message::DisksScanned);
        (app, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NextPage => {
                self.page = match self.page {
                    Page::Welcome => Page::DiskSelect,
                    Page::DiskSelect => Page::Confirm,
                    Page::Confirm => Page::Installing,
                    Page::Installing => Page::Done,
                    Page::Done => return Task::none(),
                };
                if matches!(self.page, Page::DiskSelect) {
                    return Task::perform(Self::scan_disks(), Message::DisksScanned);
                }
                Task::none()
            }
            Message::BackPage => {
                self.page = match self.page {
                    Page::Confirm => Page::DiskSelect,
                    _ => Page::Welcome,
                };
                Task::none()
            }
            Message::SelectDisk(idx) => {
                if idx < self.disks.len() {
                    self.selected_disk = Some(idx);
                    self.recipe.disk = format!("/dev/{}", self.disks[idx].name);
                }
                Task::none()
            }
            Message::DisksScanned(Ok(disks)) => {
                self.disks = disks;
                if !self.disks.is_empty() {
                    self.selected_disk = Some(0);
                    self.recipe.disk = format!("/dev/{}", self.disks[0].name);
                }
                Task::none()
            }
            Message::DisksScanned(Err(err)) => {
                self.install_log.push_str(&format!("Disk scan error: {err}\n"));
                Task::none()
            }
            Message::HostnameChanged(h) => {
                self.recipe.hostname = h;
                Task::none()
            }
            Message::StartInstall => {
                self.page = Page::Installing;
                let recipe = self.recipe.clone();
                Task::perform(Self::run_fisherman(recipe), Message::InstallFinished)
            }
            Message::InstallOutput(line) => {
                self.install_log.push_str(&line);
                self.install_log.push('\n');
                Task::none()
            }
            Message::InstallFinished(result) => {
                self.page = Page::Done;
                match result {
                    Ok(code) => {
                        self.install_ok = code == 0;
                        self.install_log.push_str(&format!(
                            "\n=== fisherman exited with code {code} ===\n"
                        ));
                    }
                    Err(e) => {
                        self.install_ok = false;
                        self.install_log.push_str(&format!("\n=== Error: {e} ===\n"));
                    }
                }
                Task::none()
            }
            Message::Quit => {
                std::process::exit(if self.install_ok { 0 } else { 1 });
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let content: Element<_> = match self.page {
            Page::Welcome => self.view_welcome(),
            Page::DiskSelect => self.view_disk_select(),
            Page::Confirm => self.view_confirm(),
            Page::Installing => self.view_installing(),
            Page::Done => self.view_done(),
        };

        // DESIGN.md: single centered column, max 760 px. Iced has no
        // max-width, so centre a width-capped container inside a filling one.
        container(
            container(content)
                .width(Length::Fixed(theme::CONTENT_MAX_WIDTH))
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .padding(theme::SPACE_L)
        .into()
    }
}

// ---- Pages ----

impl TunaInstaller {
    fn view_welcome(&self) -> Element<Message> {
        // DESIGN.md copy register: sentence case, short, factual.
        column![
            text("Install TunaOS").size(theme::TITLE),
            text("This wizard will guide you through installing TunaOS onto your computer.")
                .size(theme::BODY),
            text("You'll select a target disk, configure options, and the installer will do the rest.")
                .size(theme::BODY)
                .color(theme::TEXT_DIM),
            button("Continue")
                .on_press(Message::NextPage)
                .style(theme::button_primary),
        ]
        .spacing(theme::SPACE_M)
        .width(Length::Fill)
        .into()
    }

    fn view_disk_select(&self) -> Element<Message> {
        let mut col = Column::new()
            .spacing(theme::SPACE_S)
            .push(text("Select a target disk").size(theme::TITLE))
            .push(
                text("Everything on this disk will be erased.")
                    .size(theme::BODY)
                    .color(theme::TEXT_DIM),
            );

        if self.disks.is_empty() {
            col = col.push(
                text("Scanning disks…")
                    .size(theme::BODY)
                    .color(theme::TEXT_DIM),
            );
        } else {
            for (i, disk) in self.disks.iter().enumerate() {
                let label = format!(
                    "/dev/{}  ({}  {}) [{}]",
                    disk.name, disk.size, disk.model, disk.transport
                );
                // Device names are mono per DESIGN.md ("Fira Mono for device
                // names, refs, log output").
                let mut btn = button(text(label).font(theme::MONO_FONT).size(theme::BODY))
                    .width(Length::Fill);
                if self.selected_disk == Some(i) {
                    btn = btn.style(theme::button_primary);
                } else {
                    btn = btn.style(theme::button_secondary).on_press(Message::SelectDisk(i));
                }
                col = col.push(btn);
            }
        }

        col = col.push(
            row![
                button("Back")
                    .on_press(Message::BackPage)
                    .style(theme::button_secondary),
                iced::widget::Space::with_width(Length::Fill),
                button("Continue")
                    .on_press(Message::NextPage)
                    .style(theme::button_primary),
            ]
            .spacing(theme::SPACE_S),
        );

        col.into()
    }

    fn view_confirm(&self) -> Element<Message> {
        let disk_name = self.selected_disk.and_then(|idx| self.disks.get(idx)).map(|d| d.name.as_str()).unwrap_or("?");
        let col = column![
            text("Confirm installation").size(theme::TITLE),
            text(format!("Target Disk:  /dev/{}", disk_name)),
            text(format!("Filesystem:   {}", self.recipe.filesystem)),
            text(format!("Encryption:   {}", self.recipe.encryption.enc_type)),
            text(format!("Hostname:     {}", self.recipe.hostname)),
            text(match (&self.live_image, self.recipe.image.is_empty()) {
                (Some(live), true) => format!("Image:        {live} (this system, no download)"),
                _ => format!("Image:        {}", self.recipe.image),
            })
            .font(theme::MONO_FONT)
            .size(theme::BODY),
            // --catch marks the destructive path, per DESIGN.md.
            text("Everything on this disk will be erased.")
                .size(theme::BODY)
                .color(theme::CATCH),
            row![
                button("Back")
                    .on_press(Message::BackPage)
                    .style(theme::button_secondary),
                iced::widget::Space::with_width(Length::Fill),
                button("Install")
                    .on_press(Message::StartInstall)
                    .style(theme::button_destructive),
            ]
            .spacing(theme::SPACE_S),
        ]
        .spacing(theme::SPACE_S);

        scrollable(col).into()
    }

    fn view_installing(&self) -> Element<Message> {
        column![
            text("Installing…").size(theme::TITLE),
            scrollable(
                text(&self.install_log)
                    .size(theme::CAPTION)
                    .font(theme::MONO_FONT)
            )
            .height(Length::Fill),
        ]
        .spacing(theme::SPACE_S)
        .into()
    }

    fn view_done(&self) -> Element<Message> {
        let (icon, title, detail, color) = if self.install_ok {
            ("✓", "Installation complete", "Remove the installation media and restart your computer.", theme::SONAR)
        } else {
            ("✗", "Installation failed", "Check the installation log for details.", theme::CATCH)
        };

        column![
            text(icon).size(48).color(color),
            text(title).size(theme::TITLE),
            text(detail).size(theme::BODY).color(theme::TEXT_DIM),
            button("Restart")
                .on_press(Message::Quit)
                .style(theme::button_primary),
        ]
        .spacing(theme::SPACE_M)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .into()
    }
}

// ---- Async helpers ----

impl TunaInstaller {
    async fn scan_disks() -> Result<Vec<DiskInfo>, String> {
        let output = tokio::task::spawn_blocking(|| {
            SysCommand::new("lsblk")
                .args(&["-J", "-o", "NAME,SIZE,TYPE,MODEL,TRAN"])
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
                    let name = dev.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    let size = dev.get("size").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    let model = dev.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string();
                    let transport = dev.get("tran").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    disks.push(DiskInfo { name, size, model, transport });
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
