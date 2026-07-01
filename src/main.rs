use iced::widget::{button, column, container, horizontal_rule, row, scrollable, text, text_input, Column};
use iced::{Alignment, Application, Command, Element, Length, Settings, Theme};
use serde::{Deserialize, Serialize};
use std::process::Command as SysCommand;

fn main() -> iced::Result {
    tracing_subscriber::fmt::init();
    TunaInstaller::run(Settings {
        window: iced::window::Settings {
            size: iced::Size::new(800.0, 600.0),
            ..Default::default()
        },
        ..Default::default()
    })
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
struct Recipe {
    disk: String,
    filesystem: String,
    #[serde(default)]
    btrfs_subvolumes: bool,
    encryption: Encryption,
    image: String,
    #[serde(default)]
    target_imgref: String,
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
    HostnameChanged(String),
    StartInstall,
    InstallOutput(String),
    InstallFinished(Result<i32, String>),
    Quit,
}

struct TunaInstaller {
    page: Page,
    recipe: Recipe,
    disks: Vec<DiskInfo>,
    selected_disk: Option<usize>,
    install_log: String,
    install_ok: bool,
}

impl Application for TunaInstaller {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        let mut app = Self {
            page: Page::Welcome,
            recipe: Recipe::default(),
            disks: Vec::new(),
            selected_disk: None,
            install_log: String::new(),
            install_ok: false,
        };
        // Scan disks in background
        let cmd = Command::perform(Self::scan_disks(), Message::SelectDisk);
        (app, cmd)
    }

    fn title(&self) -> String {
        "TunaOS Installer".into()
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::NextPage => {
                self.page = match self.page {
                    Page::Welcome => Page::DiskSelect,
                    Page::DiskSelect => Page::Confirm,
                    Page::Confirm => Page::Installing,
                    Page::Installing => Page::Done,
                    Page::Done => return Command::none(),
                };
                if matches!(self.page, Page::DiskSelect) {
                    return Command::perform(Self::scan_disks(), Message::SelectDisk);
                }
                Command::none()
            }
            Message::BackPage => {
                self.page = match self.page {
                    Page::Confirm => Page::DiskSelect,
                    _ => Page::Welcome,
                };
                Command::none()
            }
            Message::SelectDisk(idx) => {
                if idx < self.disks.len() {
                    self.selected_disk = Some(idx);
                    self.recipe.disk = format!("/dev/{}", self.disks[idx].name);
                }
                Command::none()
            }
            Message::HostnameChanged(h) => {
                self.recipe.hostname = h;
                Command::none()
            }
            Message::StartInstall => {
                self.page = Page::Installing;
                let recipe = self.recipe.clone();
                Command::perform(Self::run_fisherman(recipe), Message::InstallFinished)
            }
            Message::InstallOutput(line) => {
                self.install_log.push_str(&line);
                self.install_log.push('\n');
                Command::none()
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
                Command::none()
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

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(40)
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

// ---- Pages ----

impl TunaInstaller {
    fn view_welcome(&self) -> Element<Message> {
        column![
            text("TunaOS Installer").size(32),
            text("This wizard will guide you through installing TunaOS onto your computer.").size(16),
            text("You'll select a target disk, configure options, and the installer will do the rest.").size(14),
            horizontal_rule(8),
            button("Get Started").on_press(Message::NextPage),
        ]
        .spacing(16)
        .align_items(Alignment::Center)
        .width(Length::Fill)
        .into()
    }

    fn view_disk_select(&self) -> Element<Message> {
        let mut col = Column::new()
            .spacing(12)
            .push(text("Select Target Disk").size(24))
            .push(text("All data on the selected disk will be erased.").size(14));

        if self.disks.is_empty() {
            col = col.push(text("Scanning disks...").size(16).style(iced::Color::from_rgb(0.5, 0.5, 0.5)));
        } else {
            for (i, disk) in self.disks.iter().enumerate() {
                let label = format!(
                    "/dev/{}  ({}  {}) [{}]",
                    disk.name, disk.size, disk.model, disk.transport
                );
                let mut btn = button(&label).width(Length::Fill);
                if self.selected_disk == Some(i) {
                    btn = btn.style(iced::theme::Button::Primary);
                } else {
                    btn = btn.on_press(Message::SelectDisk(i));
                }
                col = col.push(btn);
            }
        }

        col.push(horizontal_rule(8));
        col.push(
            row![
                button("Back").on_press(Message::BackPage),
                iced::widget::Space::with_width(Length::Fill),
                button("Continue")
                    .on_press(Message::NextPage)
                    .style(iced::theme::Button::Primary),
            ]
            .spacing(12),
        );

        col.into()
    }

    fn view_confirm(&self) -> Element<Message> {
        let col = column![
            text("Confirm Installation").size(24),
            horizontal_rule(8),
            text(format!("Target Disk:  /dev/{}", self.disks.get(self.selected_disk.unwrap_or(0)).map(|d| &d.name).unwrap_or(&"?"))),
            text(format!("Filesystem:   {}", self.recipe.filesystem)),
            text(format!("Encryption:   {}", self.recipe.encryption.enc_type)),
            text(format!("Hostname:     {}", self.recipe.hostname)),
            text(format!("Image:        {}", self.recipe.image)),
            horizontal_rule(8),
            text("All data on the target disk will be erased during installation.").size(14),
            row![
                button("Back").on_press(Message::BackPage),
                iced::widget::Space::with_width(Length::Fill),
                button("Install")
                    .on_press(Message::StartInstall)
                    .style(iced::theme::Button::Primary),
            ]
            .spacing(12),
        ]
        .spacing(12);

        scrollable(col).into()
    }

    fn view_installing(&self) -> Element<Message> {
        column![
            text("Installing...").size(24),
            scrollable(
                text(&self.install_log)
                    .size(12)
                    .font(iced::Font::MONOSPACE)
            )
            .height(Length::Fill),
        ]
        .spacing(12)
        .into()
    }

    fn view_done(&self) -> Element<Message> {
        let (icon, title, detail, color) = if self.install_ok {
            ("✓", "Installation Complete", "Remove the installation media and restart your computer.", [0.2, 0.8, 0.4])
        } else {
            ("✗", "Installation Failed", "Check the installation log for details.", [0.9, 0.4, 0.0])
        };

        let status = text(title).size(28).style(iced::Color::from_rgb(color[0], color[1], color[2]));

        column![
            text(icon).size(48),
            status,
            text(detail).size(14),
            horizontal_rule(8),
            button("Close").on_press(Message::Quit),
        ]
        .spacing(16)
        .align_items(Alignment::Center)
        .width(Length::Fill)
        .into()
    }
}

// ---- Async helpers ----

impl TunaInstaller {
    async fn scan_disks() -> Message {
        // Not using the return directly — this is a one-shot
        Message::SelectDisk(0)
    }

    async fn run_fisherman(recipe: Recipe) -> Result<i32, String> {
        let json = serde_json::to_string_pretty(&recipe).map_err(|e| e.to_string())?;
        let tmp = std::env::temp_dir().join("fisherman-recipe.json");
        tokio::fs::write(&tmp, &json).await.map_err(|e| e.to_string())?;

        let output = tokio::task::spawn_blocking(move || {
            SysCommand::new("fisherman")
                .arg(&tmp)
                .output()
                .map_err(|e| format!("Failed to run fisherman: {e}"))
        })
        .await
        .map_err(|e| format!("Task join error: {e}"))??;

        Ok(output.status.code().unwrap_or(-1))
    }
}
