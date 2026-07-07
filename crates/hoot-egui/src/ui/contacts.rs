use crate::image_loader::ImageLoader;
use crate::profile_metadata::{ProfileMetadata, ProfileOption};
use crate::style;
use eframe::egui::{self, Color32, Frame, Margin, RichText, ScrollArea, Stroke, TextureHandle};
use hoot_backend::{ContactDto, SenderStatusDto};
use std::collections::HashMap;
use tracing::error;

#[derive(Clone)]
pub struct Contact {
    pub pubkey: String,
    pub petname: Option<String>,
    pub metadata: ProfileMetadata,
}

impl Contact {
    fn best_name(&self) -> &str {
        self.petname
            .as_deref()
            .or(self.metadata.display_name.as_deref())
            .or(self.metadata.name.as_deref())
            .unwrap_or(&self.pubkey)
    }

    pub fn display_name(&self) -> String {
        self.best_name().to_string()
    }

    pub fn initials(&self) -> String {
        crate::style::initials_for_name(self.best_name())
    }

    pub fn picture_url(&self) -> Option<&str> {
        self.metadata
            .picture
            .as_deref()
            .filter(|url| !url.is_empty())
    }
}

pub struct ContactsManager {
    contacts: Vec<Contact>,
    image_loader: ImageLoader,
}

impl ContactsManager {
    pub fn new() -> Self {
        Self {
            contacts: Vec::new(),
            image_loader: ImageLoader::new(),
        }
    }

    pub fn set_contacts(
        &mut self,
        contacts: Vec<ContactDto>,
        profile_cache: &mut HashMap<String, ProfileOption>,
    ) {
        self.contacts = contacts
            .into_iter()
            .map(|contact| Contact {
                pubkey: contact.pubkey,
                petname: contact.petname,
                metadata: contact.metadata,
            })
            .collect();
        self.contacts.sort_by_key(contact_sort_key);
        for contact in &self.contacts {
            profile_cache.insert(
                contact.pubkey.clone(),
                ProfileOption::Some(contact.metadata.clone()),
            );
        }
    }

    pub fn upsert_metadata(&mut self, pubkey: String, metadata: ProfileMetadata) {
        if let Some(existing) = self.contacts.iter_mut().find(|c| c.pubkey == pubkey) {
            let previous_picture = existing.metadata.picture.clone();
            existing.metadata = metadata.clone();
            if previous_picture != existing.metadata.picture {
                self.image_loader.invalidate(&existing.pubkey);
            }
            self.contacts.sort_by_key(contact_sort_key);
        }
    }

    pub fn get_contacts(&self) -> &[Contact] {
        &self.contacts
    }

    pub fn find_contact(&self, pubkey: &str) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.pubkey == pubkey)
    }

    pub fn find_petname(&self, pubkey: &str) -> Option<&str> {
        self.find_contact(pubkey).and_then(|c| c.petname.as_deref())
    }

    pub fn ensure_contact_images_loaded(&mut self) {
        for contact in &self.contacts {
            if let Some(url) = contact.picture_url() {
                self.image_loader
                    .request(contact.pubkey.clone(), url.to_string());
            }
        }
    }

    pub fn process_image_queue(&mut self, ctx: &egui::Context) {
        self.image_loader.process_queue(ctx);
    }

    pub fn get_contact_image(&self, pubkey: &str) -> Option<&TextureHandle> {
        self.image_loader.get_texture(pubkey)
    }
}

fn contact_sort_key(contact: &Contact) -> String {
    contact.best_name().to_lowercase()
}

pub fn render_contacts_page(app: &mut crate::Hoot, ui: &mut egui::Ui) {
    super::page_header(ui, "Contacts");

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::BG))
        .show_inside(ui, |ui| {
            // Add contact form toggle button in a top bar
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                if style::pointer(ui.button("+ Add Contact")).clicked() {
                    app.state.contacts.show_add_form = !app.state.contacts.show_add_form;
                    app.state.contacts.add_error = None;
                }
            });
            ui.add_space(8.0);

            // Add contact form
            if app.state.contacts.show_add_form {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(style::SURFACE)
                        .stroke(Stroke::new(1.0, style::border_strong()))
                        .inner_margin(Margin::symmetric(16, 12))
                        .corner_radius(8)
                        .show(ui, |ui| {
                            ui.set_max_width(520.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Add New Contact").strong());
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label("Public Key:");
                                    style::underline_text_edit(
                                        ui,
                                        &mut app.state.contacts.add_pubkey_input,
                                        "npub1... or hex pubkey",
                                        13.5,
                                    );
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Petname:    ");
                                    style::underline_text_edit(
                                        ui,
                                        &mut app.state.contacts.add_petname_input,
                                        "Optional nickname for this contact",
                                        13.5,
                                    );
                                });

                                if let Some(err) = &app.state.contacts.add_error {
                                    ui.colored_label(Color32::RED, err.clone());
                                }

                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if style::pointer(ui.button("Save")).clicked() {
                                        let raw =
                                            app.state.contacts.add_pubkey_input.trim().to_string();
                                        let petname_raw =
                                            app.state.contacts.add_petname_input.trim().to_string();
                                        let petname = if petname_raw.is_empty() {
                                            None
                                        } else {
                                            Some(petname_raw)
                                        };

                                        if raw.is_empty() {
                                            app.state.contacts.add_error =
                                                Some("Public key is required.".to_string());
                                        } else if let Err(e) = app.backend.save_contact(raw, petname) {
                                            error!("Failed to add contact: {}", e);
                                            app.state.contacts.add_error = Some(
                                                "Failed to add contact. Use npub1... or 64-char hex."
                                                    .to_string(),
                                            );
                                        } else {
                                            app.refresh_contacts();
                                            app.state.contacts.add_pubkey_input.clear();
                                            app.state.contacts.add_petname_input.clear();
                                            app.state.contacts.show_add_form = false;
                                            app.state.contacts.add_error = None;
                                        }
                                    }
                                    if style::pointer(ui.button("Cancel")).clicked() {
                                        app.state.contacts.show_add_form = false;
                                        app.state.contacts.add_pubkey_input.clear();
                                        app.state.contacts.add_petname_input.clear();
                                        app.state.contacts.add_error = None;
                                    }
                                });
                            });
                        });
                    ui.add_space(8.0);
                });
                ui.add_space(8.0);
            }

            // Track actions to apply after the loop
            let mut contact_to_remove: Option<String> = None;
            let mut petname_to_save: Option<(String, Option<String>)> = None;

            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if app.contacts_manager.get_contacts().is_empty() {
                        super::empty_state(ui, "No contacts yet. Add one above!");
                        return;
                    }

                    app.contacts_manager.ensure_contact_images_loaded();
                    let total = app.contacts_manager.get_contacts().len();

                    for index in 0..total {
                        let contact = app.contacts_manager.get_contacts()[index].clone();

                        let is_editing =
                            app.state.contacts.editing_pubkey.as_ref() == Some(&contact.pubkey);

                        let available_width = ui.available_width();
                        let row_height = 56.0;

                        let (row_rect, _) = ui.allocate_exact_size(
                            egui::Vec2::new(available_width, row_height),
                            egui::Sense::hover(),
                        );

                        if ui.is_rect_visible(row_rect) {
                            ui.painter().rect_filled(
                                row_rect,
                                egui::CornerRadius::same(8),
                                style::SURFACE,
                            );
                            ui.painter().rect_stroke(
                                row_rect,
                                egui::CornerRadius::same(8),
                                Stroke::new(1.0, style::border()),
                                eframe::epaint::StrokeKind::Inside,
                            );
                        }

                        // Avatar
                        let avatar_size = style::AVATAR_SIZE;
                        let avatar_left = row_rect.left() + 12.0;
                        let avatar_top = row_rect.top() + (row_height - avatar_size) / 2.0;
                        let avatar_rect = egui::Rect::from_min_size(
                            egui::Pos2::new(avatar_left, avatar_top),
                            egui::Vec2::splat(avatar_size),
                        );
                        let initials = contact.initials();
                        let image = app.contacts_manager.get_contact_image(&contact.pubkey);
                        style::paint_avatar(ui.painter(), avatar_rect, &initials, image);

                        // Text — constrained to not overlap button area
                        let text_left = avatar_left + avatar_size + 10.0;
                        let max_text_w = (row_rect.right() - 170.0 - text_left - 8.0).max(0.0);
                        let display = contact.display_name();
                        let short_pubkey = if contact.pubkey.len() > 24 {
                            format!("{}…", &contact.pubkey[..24])
                        } else {
                            contact.pubkey.clone()
                        };
                        let name_galley = ui.painter().layout(
                            display,
                            egui::FontId::proportional(13.5),
                            style::TEXT,
                            max_text_w,
                        );
                        ui.painter().galley(
                            egui::Pos2::new(text_left, row_rect.top() + 14.0),
                            name_galley,
                            style::TEXT,
                        );
                        let pk_galley = ui.painter().layout(
                            short_pubkey,
                            egui::FontId::monospace(11.0),
                            style::TEXT3,
                            max_text_w,
                        );
                        ui.painter().galley(
                            egui::Pos2::new(text_left, row_rect.top() + 33.0),
                            pk_galley,
                            style::TEXT3,
                        );

                        // Action buttons — place using child_ui at fixed right position
                        let btn_area = egui::Rect::from_min_size(
                            egui::Pos2::new(row_rect.right() - 170.0, row_rect.top()),
                            egui::Vec2::new(170.0, row_height),
                        );
                        let mut btn_ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_area));
                        btn_ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if is_editing {
                                    if style::pointer(ui.button("Cancel")).clicked() {
                                        app.state.contacts.editing_pubkey = None;
                                    }
                                    if style::pointer(ui.button("Save")).clicked() {
                                        let new_petname = app
                                            .state
                                            .contacts
                                            .editing_petname_buf
                                            .trim()
                                            .to_string();
                                        let petname = if new_petname.is_empty() {
                                            None
                                        } else {
                                            Some(new_petname)
                                        };
                                        petname_to_save = Some((contact.pubkey.clone(), petname));
                                        app.state.contacts.editing_pubkey = None;
                                    }
                                } else {
                                    if style::pointer(
                                        ui.add(
                                            egui::Button::new(
                                                RichText::new("Remove")
                                                    .color(Color32::from_rgb(200, 50, 50)),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .stroke(Stroke::NONE),
                                        ),
                                    )
                                    .clicked()
                                    {
                                        contact_to_remove = Some(contact.pubkey.clone());
                                    }
                                    if style::pointer(
                                        ui.add(
                                            egui::Button::new(
                                                RichText::new("Edit").color(style::TEXT2),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .stroke(Stroke::NONE),
                                        ),
                                    )
                                    .clicked()
                                    {
                                        app.state.contacts.editing_pubkey =
                                            Some(contact.pubkey.clone());
                                        app.state.contacts.editing_petname_buf =
                                            contact.petname.clone().unwrap_or_default();
                                    }
                                }
                            },
                        );

                        // Edit petname field (shown inline when editing)
                        if is_editing {
                            let edit_area = egui::Rect::from_min_size(
                                egui::Pos2::new(text_left, row_rect.top()),
                                egui::Vec2::new(btn_area.left() - text_left - 8.0, row_height),
                            );
                            let mut edit_ui =
                                ui.new_child(egui::UiBuilder::new().max_rect(edit_area));
                            edit_ui.centered_and_justified(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(
                                        &mut app.state.contacts.editing_petname_buf,
                                    )
                                    .hint_text("Petname")
                                    .desired_width(160.0),
                                );
                            });
                        }

                        ui.add_space(4.0);
                    }

                    // Allowed Senders section
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.separator();
                    });
                    ui.add_space(8.0);

                    let allowed_response = egui::CollapsingHeader::new(
                        RichText::new("Allowed Senders").strong().color(style::TEXT),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        let allowed = match app.backend.senders_by_status(SenderStatusDto::Allowed) {
                            Ok(senders) => senders,
                            Err(e) => {
                                error!("Failed to load allowed senders: {}", e);
                                return;
                            }
                        };

                        if allowed.is_empty() {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.label(RichText::new("No allowed senders").color(style::TEXT2));
                            });
                            return;
                        }

                        let mut to_junk: Option<String> = None;
                        let mut to_remove: Option<String> = None;

                        for sender in &allowed {
                            let pubkey = &sender.pubkey;
                            let name = &sender.name;
                            let display_name = &sender.display_name;
                            let available_width = ui.available_width();
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                Frame::new()
                                    .fill(style::SURFACE)
                                    .stroke(Stroke::new(1.0, style::border_strong()))
                                    .inner_margin(Margin::symmetric(16, 8))
                                    .corner_radius(8)
                                    .show(ui, |ui| {
                                        ui.set_max_width(available_width - 16.0);
                                        ui.horizontal(|ui| {
                                            let label = display_name
                                                .as_deref()
                                                .or(name.as_deref())
                                                .unwrap_or(pubkey.as_str());
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .size(13.5)
                                                        .color(style::TEXT)
                                                        .strong(),
                                                );
                                                ui.label(
                                                    RichText::new(pubkey)
                                                        .monospace()
                                                        .small()
                                                        .color(style::TEXT3),
                                                );
                                            });

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if style::pointer(ui.button("Remove")).clicked()
                                                    {
                                                        to_remove = Some(pubkey.clone());
                                                    }
                                                    if style::pointer(ui.button("Move to Junk"))
                                                        .clicked()
                                                    {
                                                        to_junk = Some(pubkey.clone());
                                                    }
                                                },
                                            );
                                        });
                                    });
                                ui.add_space(8.0);
                            });
                            ui.add_space(4.0);
                        }

                        if let Some(pubkey) = to_junk {
                            if let Err(e) = app.backend.set_sender_status(pubkey, SenderStatusDto::Junked) {
                                error!("Failed to junk sender: {}", e);
                            } else {
                                app.refresh_requests();
                                app.refresh_junk();
                                app.refresh_inbox();
                            }
                        }

                        if let Some(pubkey) = to_remove {
                            if let Err(e) = app.backend.remove_sender_status(pubkey) {
                                error!("Failed to remove sender status: {}", e);
                            } else {
                                app.refresh_requests();
                                app.refresh_inbox();
                            }
                        }
                    });
                    let _ = style::pointer(allowed_response.header_response);

                    ui.add_space(8.0);
                });

            // Apply deferred mutations
            if let Some(pubkey) = contact_to_remove {
                if let Err(e) = app.backend.delete_contact(pubkey) {
                    error!("Failed to remove contact: {}", e);
                }
                app.refresh_contacts();
            }
            if let Some((pubkey, petname)) = petname_to_save {
                if let Err(e) = app.backend.update_contact_petname(pubkey, petname) {
                    error!("Failed to update contact petname: {}", e);
                }
                app.refresh_contacts();
            }
        });
}
