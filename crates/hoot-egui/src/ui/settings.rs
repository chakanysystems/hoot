use crate::{
    profile_metadata::{ProfileMetadata, ProfileOption},
    style, Hoot, Page,
};
use eframe::egui;
use hoot_backend::RelayConnectionStatus;
use std::cell::RefCell;
use std::collections::HashMap;
use tracing::error;

#[derive(Debug, Default)]
pub struct ProfileMetadataEditingStatus {
    display_name: String,
    editing: bool,
}

#[derive(Debug, Default)]
pub struct Nip05EditingState {
    pub new_nip05: String,
    pub verification_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SettingsSection {
    #[default]
    Identity,
    Relays,
    SmtpBridge,
    Filters,
    Notifications,
    About,
}

#[derive(Debug, Default)]
pub struct SettingsState {
    pub new_relay_url: String,
    pub metadata_state: HashMap<String, RefCell<ProfileMetadataEditingStatus>>,
    pub nip05_state: HashMap<String, RefCell<Nip05EditingState>>,
    pub settings_section: SettingsSection,
    pub show_nsec: bool,
    pub smtp_enabled: bool,
    pub smtp_host: String,
    pub smtp_port: String,
    pub smtp_user: String,
    pub smtp_password: String,
}

pub struct SettingsScreen {}

impl SettingsScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        egui::TopBottomPanel::top("settings_top_bar")
            .exact_height(48.0)
            .frame(
                egui::Frame::new()
                    .fill(style::SURFACE)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if style::pointer(
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("← Inbox")
                                    .size(13.0)
                                    .color(style::ACCENT),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                        ),
                    )
                    .clicked()
                    {
                        app.page = Page::Inbox;
                    }
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Settings")
                            .size(15.0)
                            .strong()
                            .color(style::TEXT),
                    );
                });
            });

        egui::SidePanel::left("settings_nav_panel")
            .exact_width(160.0)
            .frame(
                egui::Frame::new()
                    .fill(style::SURFACE)
                    .inner_margin(egui::Margin::symmetric(8, 8)),
            )
            .show_inside(ui, |ui| {
                ui.add_space(4.0);
                settings_nav_item(ui, app, "Identity", SettingsSection::Identity);
                settings_nav_item(ui, app, "Relays", SettingsSection::Relays);
                settings_nav_item(ui, app, "SMTP Bridge", SettingsSection::SmtpBridge);
                settings_nav_item(ui, app, "Spam & Filters", SettingsSection::Filters);
                settings_nav_item(ui, app, "Notifications", SettingsSection::Notifications);

                ui.add_space(8.0);
                ui.painter().line_segment(
                    [
                        egui::Pos2::new(ui.min_rect().left() + 8.0, ui.cursor().top()),
                        egui::Pos2::new(ui.min_rect().right() - 8.0, ui.cursor().top()),
                    ],
                    egui::Stroke::new(1.0, style::border()),
                );
                ui.add_space(8.0);

                settings_nav_item(ui, app, "About", SettingsSection::About);
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(style::BG)
                    .inner_margin(egui::Margin {
                        left: 24,
                        right: 24,
                        top: 0,
                        bottom: 0,
                    }),
            )
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(620.0);
                        ui.add_space(24.0);
                        let section = app.state.settings.settings_section.clone();
                        match section {
                            SettingsSection::Identity => render_identity(app, ui),
                            SettingsSection::Relays => render_relays(app, ui),
                            SettingsSection::SmtpBridge => render_smtp(app, ui),
                            SettingsSection::Filters => render_filters(ui),
                            SettingsSection::Notifications => render_notifications(ui),
                            SettingsSection::About => render_about(ui),
                        }
                        ui.add_space(24.0);
                    });
            });
    }
}

fn settings_nav_item(ui: &mut egui::Ui, app: &mut Hoot, label: &str, section: SettingsSection) {
    let is_active = app.state.settings.settings_section == section;
    let desired = egui::Vec2::new(ui.available_width(), 32.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if ui.is_rect_visible(rect) {
        let fill = if is_active {
            style::accent_soft()
        } else if response.hovered() {
            style::SURFACE2
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), fill);
        if is_active {
            let border_rect =
                egui::Rect::from_min_size(rect.left_top(), egui::Vec2::new(3.0, rect.height()));
            ui.painter()
                .rect_filled(border_rect, egui::CornerRadius::same(2), style::ACCENT);
        }
        let color = if is_active {
            style::ACCENT
        } else if response.hovered() {
            style::TEXT
        } else {
            style::TEXT2
        };
        ui.painter().text(
            egui::Pos2::new(rect.left() + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            color,
        );
    }
    if response.clicked() {
        app.state.settings.settings_section = section;
    }
}

fn section_heading(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(18.0)
            .strong()
            .color(style::TEXT),
    );
    ui.add_space(20.0);
}

fn setting_row(
    ui: &mut egui::Ui,
    label: &str,
    description: &str,
    widget: impl FnOnce(&mut egui::Ui),
) {
    let available_width = ui.available_width();
    ui.allocate_ui(egui::Vec2::new(available_width, 0.0), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_min_width(220.0);
                ui.set_max_width(300.0);
                ui.label(egui::RichText::new(label).size(13.5).color(style::TEXT));
                if !description.is_empty() {
                    ui.label(
                        egui::RichText::new(description)
                            .size(12.0)
                            .color(style::TEXT3),
                    );
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), widget);
        });
    });
    ui.add_space(12.0);
    ui.painter().line_segment(
        [
            egui::Pos2::new(ui.min_rect().left(), ui.cursor().top()),
            egui::Pos2::new(ui.min_rect().right(), ui.cursor().top()),
        ],
        egui::Stroke::new(1.0, style::border()),
    );
    ui.add_space(12.0);
}

fn render_identity(app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "Identity");

    setting_row(
        ui,
        "Accounts",
        "Add another account using the existing setup flow.",
        |ui| {
            if style::pointer(
                ui.add(
                    egui::Button::new(
                        egui::RichText::new("Add Account")
                            .size(12.0)
                            .color(style::ACCENT),
                    )
                    .fill(style::accent_soft())
                    .corner_radius(egui::CornerRadius::same(6)),
                ),
            )
            .clicked()
            {
                app.state.add_account_window.insert(
                    egui::Id::new(rand::random::<u32>()),
                    super::add_account_window::AddAccountWindowState::default(),
                );
            }
        },
    );

    let accounts = app.accounts.clone();
    if accounts.is_empty() {
        ui.label(
            egui::RichText::new("No account loaded.")
                .size(13.0)
                .color(style::TEXT3),
        );
        return;
    }

    let total_accounts = accounts.len();
    let mut account_to_remove: Option<String> = None;

    for (index, account) in accounts.into_iter().enumerate() {
        let pk_hex = account.pubkey_hex.clone();

        if !app.state.settings.metadata_state.contains_key(&pk_hex) {
            app.state.settings.metadata_state.insert(
                pk_hex.clone(),
                RefCell::new(ProfileMetadataEditingStatus::default()),
            );
        }

        if !app.state.settings.nip05_state.contains_key(&pk_hex) {
            app.state
                .settings
                .nip05_state
                .insert(pk_hex.clone(), RefCell::new(Nip05EditingState::default()));
        }

        let account_label = app
            .resolve_name(&pk_hex)
            .or_else(|| account.display_name.clone())
            .unwrap_or_else(|| format!("Account {}", index + 1));
        let npub = account.npub.clone();
        let profile_metadata =
            crate::profile_metadata::get_profile_metadata(app, pk_hex.clone()).clone();

        ui.label(
            egui::RichText::new(account_label)
                .size(14.5)
                .strong()
                .color(style::TEXT),
        );
        ui.add_space(8.0);

        setting_row(ui, "Display name", "Shown to people you message.", |ui| {
            let key_meta_state = app
                .state
                .settings
                .metadata_state
                .get(&pk_hex)
                .expect("State should exist");

            let mut save_clicked = false;
            let mut edit_clicked = false;
            let mut cancel_clicked = false;
            let mut new_name_to_save: Option<String> = None;

            {
                let mut meta_state = key_meta_state.borrow_mut();
                let is_editing = meta_state.editing;

                if is_editing {
                    ui.add(
                        egui::TextEdit::singleline(&mut meta_state.display_name)
                            .desired_width(160.0),
                    );
                    if style::pointer(
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("Save").size(12.0).color(style::ACCENT),
                            )
                            .fill(style::accent_soft())
                            .corner_radius(egui::CornerRadius::same(6)),
                        ),
                    )
                    .clicked()
                    {
                        save_clicked = true;
                        new_name_to_save = Some(meta_state.display_name.clone());
                    }
                    if style::pointer(
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("Cancel").size(12.0).color(style::TEXT2),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                        ),
                    )
                    .clicked()
                    {
                        cancel_clicked = true;
                    }
                } else {
                    let display_text = match &profile_metadata {
                        ProfileOption::Some(meta) => meta
                            .display_name
                            .clone()
                            .unwrap_or_else(|| "Not set".to_string()),
                        ProfileOption::Waiting => "Loading...".to_string(),
                    };
                    ui.label(
                        egui::RichText::new(&display_text)
                            .size(13.0)
                            .color(style::TEXT2),
                    );
                    if style::pointer(
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("Edit").size(12.0).color(style::ACCENT),
                            )
                            .fill(style::accent_soft())
                            .corner_radius(egui::CornerRadius::same(6)),
                        ),
                    )
                    .clicked()
                    {
                        edit_clicked = true;
                    }
                }

                if edit_clicked {
                    meta_state.editing = true;
                }
                if cancel_clicked {
                    meta_state.editing = false;
                }
                if save_clicked {
                    meta_state.editing = false;
                }
            }

            if save_clicked {
                if let Some(new_name) = new_name_to_save {
                    let mut new_meta = match &profile_metadata {
                        ProfileOption::Some(meta) => meta.to_owned(),
                        ProfileOption::Waiting => ProfileMetadata::default(),
                    };
                    new_meta.display_name = Some(new_name);
                    if let Err(e) = crate::profile_metadata::update_logged_in_profile_metadata(
                        app,
                        pk_hex.clone(),
                        new_meta,
                    ) {
                        tracing::error!("Couldn't update profile: {}", e);
                    }
                }
            }
        });

        setting_row(
            ui,
            "Your address (npub)",
            "Share this with people who want to message you.",
            |ui| {
                if style::pointer(
                    ui.add(
                        egui::Button::new(
                            egui::RichText::new("Copy").size(12.0).color(style::ACCENT),
                        )
                        .fill(style::accent_soft())
                        .corner_radius(egui::CornerRadius::same(6)),
                    ),
                )
                .clicked()
                {
                    ui.ctx().copy_text(npub.clone());
                }
                ui.add_space(8.0);
                let short = if npub.len() > 16 { &npub[..16] } else { &npub };
                ui.label(egui::RichText::new(short).size(12.0).color(style::TEXT3));
            },
        );

        ui.label(
            egui::RichText::new("NIP-05 Identifiers")
                .size(13.5)
                .color(style::TEXT),
        );
        ui.add_space(4.0);

        let nip05s = match app.backend.get_nip05s_for_pubkey(pk_hex.clone()) {
            Ok(entries) => entries,
            Err(e) => {
                error!("Failed to get NIP-05s: {}", e);
                Vec::new()
            }
        };

        for entry in &nip05s {
            ui.horizontal(|ui| {
                let (icon, color, _tooltip) = crate::ui::nip05_status::status_display(entry);
                ui.colored_label(color, icon);
                ui.label(
                    egui::RichText::new(&entry.nip05)
                        .size(13.0)
                        .color(style::TEXT),
                );
                if entry.is_own {
                    ui.label(egui::RichText::new("(own)").size(11.0).color(style::TEXT3));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if style::pointer(
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("Remove").size(12.0).color(style::TEXT2),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                        ),
                    )
                    .clicked()
                    {
                        if let Err(e) = app
                            .backend
                            .delete_nip05(pk_hex.clone(), entry.nip05.clone())
                        {
                            error!("Failed to delete NIP-05: {}", e);
                        }
                    }
                    if style::pointer(
                        ui.add(
                            egui::Button::new(
                                egui::RichText::new("Verify")
                                    .size(12.0)
                                    .color(style::ACCENT),
                            )
                            .fill(style::accent_soft())
                            .corner_radius(egui::CornerRadius::same(6)),
                        ),
                    )
                    .clicked()
                    {
                        if let Err(e) =
                            app.backend
                                .add_nip05(pk_hex.clone(), entry.nip05.clone(), entry.is_own)
                        {
                            error!("Failed to queue NIP-05 verification: {}", e);
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }

        if nip05s.is_empty() {
            ui.label(
                egui::RichText::new("No NIP-05 identifiers added yet.")
                    .size(12.0)
                    .color(style::TEXT3),
            );
            ui.add_space(4.0);
        }

        ui.horizontal(|ui| {
            let nip05_state = app
                .state
                .settings
                .nip05_state
                .get(&pk_hex)
                .expect("State should exist");
            let mut verify_clicked = false;
            let new_nip05_value;
            {
                let mut state = nip05_state.borrow_mut();
                ui.add(
                    egui::TextEdit::singleline(&mut state.new_nip05)
                        .hint_text("user@domain.com")
                        .desired_width(200.0),
                );
                new_nip05_value = state.new_nip05.clone();
                if style::pointer(
                    ui.add(
                        egui::Button::new(
                            egui::RichText::new("Verify & Add")
                                .size(12.0)
                                .color(style::ACCENT),
                        )
                        .fill(style::accent_soft())
                        .corner_radius(egui::CornerRadius::same(6)),
                    ),
                )
                .clicked()
                {
                    verify_clicked = true;
                }
                if let Some(error) = &state.verification_error {
                    ui.label(
                        egui::RichText::new(error)
                            .size(12.0)
                            .color(egui::Color32::RED),
                    );
                }
            }
            if verify_clicked && !new_nip05_value.is_empty() {
                let mut state = nip05_state.borrow_mut();
                let Some(nip05) = hoot_backend::normalize_nip05_identifier(&new_nip05_value) else {
                    state.verification_error =
                        Some("Invalid NIP-05 format. Use: user@domain.com".to_string());
                    return;
                };
                if let Err(e) = app.backend.add_nip05(pk_hex.clone(), nip05, true) {
                    state.verification_error = Some(format!("Failed to save: {}", e));
                } else {
                    state.new_nip05.clear();
                    state.verification_error = None;
                }
            }
        });

        ui.add_space(16.0);

        setting_row(
            ui,
            "Danger zone",
            "Permanently remove this key from Hoot.",
            |ui| {
                if style::pointer(
                    ui.add(
                        egui::Button::new(
                            egui::RichText::new("Remove Key")
                                .size(12.0)
                                .color(egui::Color32::RED),
                        )
                        .fill(egui::Color32::from_rgba_unmultiplied(220, 38, 38, 20))
                        .corner_radius(egui::CornerRadius::same(6)),
                    ),
                )
                .clicked()
                {
                    account_to_remove = Some(pk_hex.clone());
                }
            },
        );

        if index + 1 < total_accounts {
            ui.add_space(4.0);
            ui.painter().line_segment(
                [
                    egui::Pos2::new(ui.min_rect().left(), ui.cursor().top()),
                    egui::Pos2::new(ui.min_rect().right(), ui.cursor().top()),
                ],
                egui::Stroke::new(1.0, style::border()),
            );
            ui.add_space(16.0);
        }
    }

    if let Some(pubkey) = account_to_remove {
        app.delete_account_and_refresh(pubkey);
    }
}

fn render_relays(app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "Relays");

    ui.label(
        egui::RichText::new(
            "A relay is a server that Hoot connects with to send & receive messages.",
        )
        .size(13.0)
        .color(style::TEXT2),
    );
    ui.add_space(16.0);

    let mut relay_to_remove: Option<String> = None;

    match app.backend.relay_statuses() {
        Ok(relays) => {
            for relay in relays {
                ui.horizontal(|ui| {
                    let dot_color = match relay.status {
                        RelayConnectionStatus::Connected => style::GREEN,
                        RelayConnectionStatus::Connecting => style::AMBER,
                        RelayConnectionStatus::Disconnected => egui::Color32::from_rgb(220, 38, 38),
                    };
                    let (dot_rect, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(dot_rect.center(), 4.0, dot_color);

                    ui.label(
                        egui::RichText::new(&relay.url)
                            .size(13.0)
                            .color(style::TEXT),
                    );

                    ui.label(
                        egui::RichText::new(format!("({:?})", relay.status))
                            .size(12.0)
                            .color(style::TEXT3),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if style::pointer(
                            ui.add(
                                egui::Button::new(
                                    egui::RichText::new("Remove").size(12.0).color(style::TEXT2),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                            ),
                        )
                        .clicked()
                        {
                            relay_to_remove = Some(relay.url.clone());
                        }
                    });
                });
                ui.add_space(8.0);
                ui.painter().line_segment(
                    [
                        egui::Pos2::new(ui.min_rect().left(), ui.cursor().top()),
                        egui::Pos2::new(ui.min_rect().right(), ui.cursor().top()),
                    ],
                    egui::Stroke::new(1.0, style::border()),
                );
                ui.add_space(8.0);
            }
        }
        Err(e) => {
            error!("Failed to list relays: {}", e);
            ui.colored_label(egui::Color32::RED, "Failed to load relays.");
        }
    }

    if let Some(url) = relay_to_remove {
        app.remove_relay_url(url);
    }

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.state.settings.new_relay_url)
                .hint_text("wss://relay.example.com")
                .desired_width(280.0),
        );
        if style::pointer(
            ui.add(
                egui::Button::new(egui::RichText::new("Add Relay").color(style::ACCENT))
                    .fill(style::accent_soft())
                    .corner_radius(egui::CornerRadius::same(6)),
            ),
        )
        .clicked()
            && !app.state.settings.new_relay_url.is_empty()
        {
            let url = app.state.settings.new_relay_url.clone();
            app.add_relay_url(url);
            app.state.settings.new_relay_url.clear();
        }
    });
}

fn render_smtp(_app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "SMTP Bridge");

    egui::Frame::new()
        .fill(style::SURFACE)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("SMTP Server")
                    .size(13.5)
                    .color(style::TEXT),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Not added yet. Connect an SMTP server to send and receive from regular email addresses.")
                    .size(13.0)
                    .color(style::TEXT2),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Coming soon.")
                    .size(12.0)
                    .color(style::TEXT3),
            );
        });
}

fn render_filters(ui: &mut egui::Ui) {
    section_heading(ui, "Spam & Filters");
    ui.label(
        egui::RichText::new("Spam filter settings coming soon.")
            .size(13.0)
            .color(style::TEXT2),
    );
}

fn render_notifications(ui: &mut egui::Ui) {
    section_heading(ui, "Notifications");
    ui.label(
        egui::RichText::new("Notification settings coming soon.")
            .size(13.0)
            .color(style::TEXT2),
    );
}

fn render_about(ui: &mut egui::Ui) {
    section_heading(ui, "About");
    ui.label(
        egui::RichText::new(format!("Hoot v{}", env!("CARGO_PKG_VERSION")))
            .size(13.0)
            .color(style::TEXT),
    );
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "Built on the Nostr protocol. An open, censorship-resistant messaging network.",
        )
        .size(13.0)
        .color(style::TEXT2),
    );
    ui.add_space(16.0);
    ui.label(
        egui::RichText::new("https://github.com/chakanysystems/hoot")
            .size(12.0)
            .color(style::TEXT3),
    );
    ui.add_space(32.0);
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new("A product by")
                .size(12.0)
                .color(style::TEXT3),
        );
        ui.add_space(6.0);
        ui.add(
            egui::Image::from_bytes(
                "bytes://chakany-logo.svg",
                include_bytes!("../../assets/chakany-logo.svg"),
            )
            .fit_to_exact_size(egui::Vec2::new(120.0, 24.0)),
        );
    });
}
