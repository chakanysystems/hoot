use crate::image_loader::ImageLoader;
use crate::profile_metadata::{ProfileMetadata, ProfileOption};
use eframe::egui::{
    self, Align2, Color32, FontId, Frame, Margin, RichText, ScrollArea, Sense, Stroke,
    TextureHandle, Vec2,
};
use hoot_backend::ContactDto;
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
        let fallback = self.best_name();
        let mut initials = fallback
            .split_whitespace()
            .filter_map(|segment| segment.chars().next())
            .map(|ch| ch.to_ascii_uppercase())
            .take(2)
            .collect::<String>();
        if initials.is_empty() {
            initials = fallback
                .chars()
                .take(2)
                .map(|ch| ch.to_ascii_uppercase())
                .collect();
        }
        initials
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
        self.contacts
            .sort_by(|a, b| contact_sort_key(a).cmp(&contact_sort_key(b)));
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
            self.contacts
                .sort_by(|a, b| contact_sort_key(a).cmp(&contact_sort_key(b)));
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
    use crate::style;

    ui.horizontal(|ui| {
        ui.heading("Contacts");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+ Add Contact").clicked() {
                app.state.contacts.show_add_form = !app.state.contacts.show_add_form;
                app.state.contacts.add_error = None;
            }
        });
    });

    ui.add_space(8.0);

    if app.state.contacts.show_add_form {
        Frame::none()
            .fill(style::CARD_BG)
            .stroke(Stroke::new(1.0, style::CARD_STROKE))
            .inner_margin(Margin::symmetric(16, 12))
            .corner_radius(8)
            .show(ui, |ui| {
                ui.label(RichText::new("Add New Contact").strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Public Key:");
                    ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut app.state.contacts.add_pubkey_input)
                            .hint_text("npub1... or hex pubkey"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Petname:     ");
                    ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut app.state.contacts.add_petname_input)
                            .hint_text("Optional nickname for this contact"),
                    );
                });
                if let Some(err) = &app.state.contacts.add_error {
                    ui.colored_label(Color32::RED, err.clone());
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        let pubkey = app.state.contacts.add_pubkey_input.trim().to_string();
                        let petname_raw = app.state.contacts.add_petname_input.trim().to_string();
                        let petname = if petname_raw.is_empty() {
                            None
                        } else {
                            Some(petname_raw)
                        };
                        if app.contacts_manager.find_contact(&pubkey).is_some() {
                            app.state.contacts.add_error = Some("Contact already exists.".to_string());
                        } else if let Err(e) = app.backend.save_contact(pubkey.clone(), petname) {
                            error!("Failed to add contact: {}", e);
                            app.state.contacts.add_error = Some("Failed to add contact.".to_string());
                        } else {
                            app.refresh_contacts();
                            app.state.contacts.add_pubkey_input.clear();
                            app.state.contacts.add_petname_input.clear();
                            app.state.contacts.show_add_form = false;
                            app.state.contacts.add_error = None;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        app.state.contacts.show_add_form = false;
                        app.state.contacts.add_pubkey_input.clear();
                        app.state.contacts.add_petname_input.clear();
                        app.state.contacts.add_error = None;
                    }
                });
            });
        ui.add_space(8.0);
    }

    if app.contacts_manager.get_contacts().is_empty() {
        ui.label("No contacts yet. Add one above!");
        return;
    }

    let mut contact_to_remove: Option<String> = None;
    let mut petname_to_save: Option<(String, Option<String>)> = None;

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let total = app.contacts_manager.get_contacts().len();
            for index in 0..total {
                let contact = app.contacts_manager.get_contacts()[index].clone();
                app.contacts_manager.ensure_contact_images_loaded();
                let is_editing = app.state.contacts.editing_pubkey.as_ref() == Some(&contact.pubkey);
                Frame::none()
                    .fill(style::CARD_BG)
                    .stroke(Stroke::new(1.0, style::CARD_STROKE))
                    .inner_margin(Margin::symmetric(16, 12))
                    .corner_radius(8)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            draw_contact_avatar(&app.contacts_manager, ui, &contact);
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                if is_editing {
                                    ui.horizontal(|ui| {
                                        ui.label("Petname:");
                                        ui.text_edit_singleline(&mut app.state.contacts.editing_petname_buf);
                                        if ui.button("Save").clicked() {
                                            let new_petname = app.state.contacts.editing_petname_buf.trim().to_string();
                                            let petname = if new_petname.is_empty() { None } else { Some(new_petname) };
                                            petname_to_save = Some((contact.pubkey.clone(), petname));
                                            app.state.contacts.editing_pubkey = None;
                                        }
                                        if ui.button("Cancel").clicked() {
                                            app.state.contacts.editing_pubkey = None;
                                        }
                                    });
                                } else {
                                    ui.label(RichText::new(contact.display_name()).strong());
                                    ui.label(RichText::new(&contact.pubkey).monospace().small().color(style::TEXT_MUTED));
                                }
                            });
                            if !is_editing {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(RichText::new("X").color(Color32::RED)).clicked() {
                                        contact_to_remove = Some(contact.pubkey.clone());
                                    }
                                    if ui.button("Edit").clicked() {
                                        app.state.contacts.editing_pubkey = Some(contact.pubkey.clone());
                                        app.state.contacts.editing_petname_buf = contact.petname.clone().unwrap_or_default();
                                    }
                                });
                            }
                        });
                    });
                ui.add_space(4.0);
            }
        });

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
}

fn draw_contact_avatar(manager: &ContactsManager, ui: &mut egui::Ui, contact: &Contact) {
    use crate::style;
    let size = Vec2::splat(style::AVATAR_SIZE);
    if let Some(texture) = manager.get_contact_image(&contact.pubkey) {
        ui.add(egui::Image::new((texture.id(), size)).maintain_aspect_ratio(true));
        return;
    }
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.circle_filled(rect.center(), style::AVATAR_SIZE / 2.0, style::ACCENT);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        contact.initials(),
        FontId::proportional(18.0),
        Color32::WHITE,
    );
}
