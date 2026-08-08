//! Views, built from the cosmic widget set and the cosmic-theme palette.
//!
//! Nothing here hardcodes a colour or a pixel gap: colours come from
//! `cosmic::theme::active().cosmic()` and spacing from its `spacing` scale, so
//! the installer follows the user's COSMIC theme like every other COSMIC app.

use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;

use crate::{
    available_encryption_choices, product, Message, Page, TunaInstaller, FILESYSTEMS,
};

pub fn view(app: &TunaInstaller) -> Element<'_, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;

    // Deliberately renders nothing, so the capture harness's pixel gate can be
    // calibrated against a measured blank frame instead of a guessed one. This
    // is how the numbers in `capture.rs` were derived at the broken end, and
    // re-running it is how they should be re-derived if the UI changes:
    //
    //     TUNA_BLANK_SELFTEST=1 TUNA_CAPTURE_DIR=/tmp/blank just capture
    //
    // It must FAIL. If it ever passes, the gate has stopped detecting a page
    // that did not render, which is the entire point of the gate.
    if std::env::var_os("TUNA_BLANK_SELFTEST").is_some() {
        return widget::container(widget::space::horizontal())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    let content: Element<Message> = match app.page() {
        Page::Welcome => welcome(app),
        Page::DiskSelect => disk_select(app),
        Page::Options => options(app),
        Page::Confirm => confirm(app),
        Page::Installing => installing(app),
        Page::Done => done(app),
    };

    widget::container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(spacing.space_l)
        .into()
}

/// Back / spacer / forward, the shape every COSMIC wizard uses.
fn nav_row<'a>(
    back: Option<Message>,
    forward: Option<(&'a str, Message, bool)>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let mut row = widget::row::with_capacity(3).spacing(spacing.space_s);

    if let Some(msg) = back {
        row = row.push(widget::button::standard("Back").on_press(msg));
    }
    row = row.push(widget::space::horizontal());
    if let Some((label, msg, destructive)) = forward {
        let button = if destructive {
            widget::button::destructive(label)
        } else {
            widget::button::suggested(label)
        };
        row = row.push(button.on_press(msg));
    }
    row.align_y(Alignment::Center).into()
}

fn welcome(_app: &TunaInstaller) -> Element<'_, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;

    let hero = widget::column::with_children(vec![
        widget::icon::from_name("drive-harddisk-symbolic")
            .size(64)
            .icon()
            .into(),
        widget::text::title1(format!("Install {}", product::name())).into(),
        widget::text::body(format!(
            "Welcome. This assistant will guide you through installing {} onto this \
             computer.",
            product::name()
        ))
        .into(),
        widget::text::caption(
            "You will choose a target disk and a few options. Nothing is written to any \
             disk until you confirm on the last step.",
        )
        .into(),
    ])
    .spacing(spacing.space_s)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    widget::column::with_children(vec![
        widget::space::vertical().into(),
        hero.into(),
        widget::space::vertical().into(),
        nav_row(None, Some(("Get started", Message::NextPage, false))),
    ])
    .spacing(spacing.space_m)
    .height(Length::Fill)
    .into()
}

fn disk_select(app: &TunaInstaller) -> Element<'_, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;

    let body: Element<Message> = if app.disks().is_empty() {
        widget::column::with_children(vec![
            widget::progress_bar::indeterminate_linear()
                .width(Length::Fill)
                .into(),
            widget::text::body("Scanning for disks…").into(),
        ])
        .spacing(spacing.space_s)
        .into()
    } else {
        let mut list = widget::list_column().style(cosmic::theme::Container::List);
        for (i, disk) in app.disks().iter().enumerate() {
            let _selected = app.selected_disk() == Some(i);

            let label = widget::column::with_children(vec![
                widget::text::body(format!("/dev/{}", disk.name)).into(),
                widget::text::caption(format!(
                    "{} · {} · {}",
                    disk.size,
                    if disk.model.is_empty() {
                        "unknown model"
                    } else {
                        &disk.model
                    },
                    if disk.transport.is_empty() {
                        "unknown bus"
                    } else {
                        &disk.transport
                    },
                ))
                .into(),
            ])
            .spacing(spacing.space_xxxs)
            .width(Length::Fill);

            let row = widget::row::with_children(vec![
                label.into(),
                widget::radio(widget::text::body(""), i, app.selected_disk(), Message::SelectDisk)
                    .into(),
            ])
            .align_y(Alignment::Center)
            .spacing(spacing.space_s);

            list = list.add(
                widget::mouse_area(row)
                    .on_press(Message::SelectDisk(i)),
            );
        }
        widget::scrollable(list.into_element())
            .height(Length::Fill)
            .into()
    };

    page_frame(
        "Select a disk",
        "Everything on the disk you pick will be erased.",
        body,
        nav_row(
            Some(Message::BackPage),
            app.selected_disk()
                .map(|_| ("Continue", Message::NextPage, false)),
        ),
    )
}

fn options(app: &TunaInstaller) -> Element<'_, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let recipe = app.recipe();

    let fs_index = FILESYSTEMS.iter().position(|f| *f == recipe.filesystem);

    // tunaOS#734: this list is `has_tpm`-filtered, same set XFCE offers on
    // this hardware, and `idx` below is a position in it — NOT in the full
    // `ENCRYPTION_CHOICES` table. `update()`'s `EncryptionChanged` handler
    // rebuilds the identical filtered list before indexing into it, so the
    // two stay in lockstep without passing the list through a message.
    let enc_choices = available_encryption_choices(app.has_tpm());
    let enc_index = enc_choices
        .iter()
        .position(|c| c.id == recipe.encryption.enc_type);
    let enc_labels: Vec<&str> = enc_choices.iter().map(|c| c.label).collect();
    let enc_description = enc_index
        .and_then(|i| enc_choices.get(i))
        .map(|c| c.description)
        .unwrap_or_default();
    // "luks-passphrase" and "tpm2-luks-passphrase" both contain this
    // substring; bare "tpm2-luks" and "none" don't. Same test `encryption_ok()`
    // uses to gate Continue below, so the field's presence and the field's
    // requiredness can never disagree.
    let needs_passphrase = recipe.encryption.enc_type.contains("passphrase");

    let system = widget::settings::section()
        .title("System")
        .add(widget::settings::item(
            "Computer name",
            widget::text_input("tunaos", &recipe.hostname)
                .on_input(Message::HostnameChanged)
                .width(Length::Fixed(260.0)),
        ))
        .add(widget::settings::item(
            "Filesystem",
            widget::dropdown(&FILESYSTEMS, fs_index, Message::FilesystemChanged),
        ));

    let mut security = widget::settings::section().title("Encryption").add(
        widget::settings::item::builder("Disk encryption")
            .description(enc_description)
            // Handed over by value, not as `.as_slice()`: the returned
            // `Element` outlives this function, so a borrow of the local
            // `Vec` would not compile (E0515). `Vec<&'static str>` converts
            // into an owned `Cow<[&str]>`, which the dropdown keeps.
            .control(widget::dropdown(
                enc_labels,
                enc_index,
                Message::EncryptionChanged,
            )),
    );

    // Only build the passphrase controls once the chosen encryption actually
    // carries one. The KDE sibling crashed precisely at this kind of
    // conditional: its constructor called setChecked() before the passphrase
    // widgets existed, so the toggled handler ran against uninitialised
    // pointers. In Rust the equivalent mistake cannot compile, but the
    // conditional is still the honest UI — and "tpm2-luks" alone must NOT show
    // this field, since fisherman never reads a passphrase for it.
    if needs_passphrase {
        security = security.add(widget::settings::item(
            "Passphrase",
            widget::secure_input(
                "Required to unlock at boot",
                &recipe.encryption.passphrase,
                Some(Message::TogglePassphraseVisible),
                app.passphrase_hidden(),
            )
            .on_input(Message::PassphraseChanged)
            .width(Length::Fixed(260.0)),
        ));
    }

    let body = widget::scrollable(
        widget::settings::view_column(vec![system.into(), security.into()])
            .spacing(spacing.space_m),
    )
    .height(Length::Fill);

    page_frame(
        "Options",
        "Sensible defaults are already chosen. Change them only if you need to.",
        body.into(),
        nav_row(
            Some(Message::BackPage),
            // Blocked while a passphrase-carrying choice has an empty
            // passphrase, so this can never reach Confirm/Install and only
            // then discover fisherman rejects the recipe (tunaOS#734).
            app.encryption_ok()
                .then_some(("Continue", Message::NextPage, false)),
        ),
    )
}

fn confirm(app: &TunaInstaller) -> Element<'_, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let recipe = app.recipe();

    let disk = app
        .selected_disk()
        .and_then(|i| app.disks().get(i))
        .map_or_else(|| "?".to_string(), |d| format!("/dev/{}", d.name));

    let image = match (app.live_image(), recipe.image.is_empty()) {
        (Some(live), true) => format!("{live} (this system — no download)"),
        _ => recipe.image.clone(),
    };

    let summary = widget::settings::section()
        .title("Summary")
        .add(widget::settings::item("Target disk", widget::text::body(disk)))
        .add(widget::settings::item(
            "Filesystem",
            widget::text::body(recipe.filesystem.clone()),
        ))
        .add(widget::settings::item(
            "Encryption",
            widget::text::body(recipe.encryption.enc_type.clone()),
        ))
        .add(widget::settings::item(
            "Computer name",
            widget::text::body(recipe.hostname.clone()),
        ))
        .add(widget::settings::item("Image", widget::text::body(image)));

    // Not `widget::warning::warning`: its filled amber background renders the
    // body text near-invisible on the dark COSMIC palette, which the first
    // screenshot run made obvious and no diff ever would have. A card with
    // warning-coloured text keeps the emphasis and stays legible in both
    // light and dark.
    let theme = cosmic::theme::active();
    let warning = widget::container(
        widget::row::with_children(vec![
            widget::icon::from_name("dialog-warning-symbolic")
                .size(16)
                .icon()
                .into(),
            widget::text::body(
                "Everything on the target disk will be erased. This cannot be undone.",
            )
            .class(cosmic::theme::Text::Color(
                theme.cosmic().warning_text_color().into(),
            ))
            .into(),
        ])
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center),
    )
    .class(cosmic::theme::Container::Card)
    .padding(spacing.space_s)
    .width(Length::Fill);

    let body = widget::scrollable(
        widget::column::with_children(vec![warning.into(), summary.into()])
            .spacing(spacing.space_m),
    )
    .height(Length::Fill);

    page_frame(
        "Confirm",
        "The last screen before anything is written.",
        body.into(),
        nav_row(
            Some(Message::BackPage),
            Some(("Install", Message::StartInstall, true)),
        ),
    )
}

fn installing(app: &TunaInstaller) -> Element<'_, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let theme = cosmic::theme::active();
    let cosmic_theme = theme.cosmic();

    let log = widget::container(
        widget::scrollable(
            widget::text::monotext(app.install_log())
                .size(12)
                .width(Length::Fill),
        )
        .height(Length::Fill),
    )
    .class(cosmic::theme::Container::Card)
    .padding(spacing.space_s)
    .width(Length::Fill)
    .height(Length::Fill);

    let body = widget::column::with_children(vec![
        widget::progress_bar::indeterminate_linear()
            .width(Length::Fill)
            .into(),
        log.into(),
        widget::text::caption("Do not power off the computer.")
            .class(cosmic::theme::Text::Color(cosmic_theme.warning_text_color().into()))
            .into(),
    ])
    .spacing(spacing.space_s)
    .height(Length::Fill);

    page_frame(
        format!("Installing {}", product::name()),
        "fisherman is writing the image to disk.",
        body.into(),
        widget::space::horizontal().into(),
    )
}

fn done(app: &TunaInstaller) -> Element<'_, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let theme = cosmic::theme::active();
    let cosmic_theme = theme.cosmic();

    let (icon, title, detail, colour) = if app.install_ok() {
        (
            "emblem-ok-symbolic",
            "Installation complete",
            "Remove the installation media and restart the computer.",
            cosmic_theme.success_text_color(),
        )
    } else {
        (
            "dialog-error-symbolic",
            "Installation failed",
            "The install log above has the details.",
            cosmic_theme.destructive_text_color(),
        )
    };

    let hero = widget::column::with_children(vec![
        widget::icon::from_name(icon).size(64).icon().into(),
        widget::text::title2(title)
            .class(cosmic::theme::Text::Color(colour.into()))
            .into(),
        widget::text::body(detail).into(),
    ])
    .spacing(spacing.space_s)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    widget::column::with_children(vec![
        widget::space::vertical().into(),
        hero.into(),
        widget::space::vertical().into(),
        nav_row(None, Some(("Close", Message::Quit, false))),
    ])
    .spacing(spacing.space_m)
    .height(Length::Fill)
    .into()
}

/// Title, subtitle, scrolling body, navigation footer.
fn page_frame<'a>(
    title: impl Into<std::borrow::Cow<'a, str>> + 'a,
    subtitle: impl Into<std::borrow::Cow<'a, str>> + 'a,
    body: Element<'a, Message>,
    footer: Element<'a, Message>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    widget::column::with_children(vec![
        widget::text::title2(title).into(),
        widget::text::caption(subtitle).into(),
        widget::divider::horizontal::default().into(),
        body,
        widget::divider::horizontal::default().into(),
        footer,
    ])
    .spacing(spacing.space_s)
    .height(Length::Fill)
    .into()
}
