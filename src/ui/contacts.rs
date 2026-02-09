use crate::db::Db;
use crate::image_loader::ImageLoader;
use crate::profile_metadata::ProfileMetadata;
use crate::profile_metadata::ProfileOption;
use eframe::egui::{
    self, Align2, Color32, FontId, Frame, Margin, RichText, ScrollArea, Sense, Stroke,
    TextureHandle, Vec2,
};
use std::collections::HashMap;
use tracing::error;

#[derive(Clone)]
pub struct Contact {
    pub pubkey: String,
    pub petname: Option<String>,
    pub metadata: ProfileMetadata,
}

impl Contact {
    /// Best display name: petname > display_name > name > pubkey, without cloning.
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

    pub fn load_from_db(
        &mut self,
        db: &Db,
        profile_cache: &mut HashMap<String, ProfileOption>,
    ) -> anyhow::Result<()> {
        let contacts_data = db.get_user_contacts()?;

        self.contacts = contacts_data
            .into_iter()
            .map(|(pubkey, petname, metadata)| Contact {
                pubkey,
                petname,
                metadata,
            })
            .collect();

        self.contacts
            .sort_by(|a, b| contact_sort_key(a).cmp(&contact_sort_key(b)));

        // Cache metadata in profile_cache
        for contact in &self.contacts {
            profile_cache.insert(
                contact.pubkey.clone(),
                ProfileOption::Some(contact.metadata.clone()),
            );
        }

        Ok(())
    }

    pub fn add_contact(
        &mut self,
        db: &Db,
        pubkey: String,
        petname: Option<String>,
        metadata: ProfileMetadata,
    ) -> anyhow::Result<()> {
        if self.contacts.iter().any(|c| c.pubkey == pubkey) {
            return Ok(());
        }

        db.save_contact(&pubkey, petname.as_deref())?;

        self.contacts.push(Contact {
            pubkey: pubkey.clone(),
            petname,
            metadata,
        });
        self.contacts
            .sort_by(|a, b| contact_sort_key(a).cmp(&contact_sort_key(b)));

        Ok(())
    }

    pub fn remove_contact(&mut self, db: &Db, pubkey: &str) -> anyhow::Result<()> {
        db.delete_contact(pubkey)?;

        self.contacts.retain(|c| c.pubkey != pubkey);
        self.image_loader.invalidate(pubkey);

        Ok(())
    }

    pub fn update_petname(
        &mut self,
        db: &Db,
        pubkey: &str,
        petname: Option<String>,
    ) -> anyhow::Result<()> {
        db.update_contact_petname(pubkey, petname.as_deref())?;

        if let Some(contact) = self.contacts.iter_mut().find(|c| c.pubkey == pubkey) {
            contact.petname = petname;
        }
        self.contacts
            .sort_by(|a, b| contact_sort_key(a).cmp(&contact_sort_key(b)));

        Ok(())
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

    // Add contact form
    if app.state.contacts.show_add_form {
        Frame::none()
            .fill(style::CARD_BG)
            .stroke(Stroke::new(1.0, style::CARD_STROKE))
            .inner_margin(Margin::symmetric(16.0, 12.0))
            .rounding(8.0)
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
                        let raw = app.state.contacts.add_pubkey_input.trim().to_string();
                        let petname_raw = app.state.contacts.add_petname_input.trim().to_string();
                        let petname = if petname_raw.is_empty() {
                            None
                        } else {
                            Some(petname_raw)
                        };

                        // Try parsing the pubkey
                        use nostr::FromBech32;
                        let parsed = nostr::PublicKey::from_bech32(&raw)
                            .or_else(|_| nostr::PublicKey::from_hex(&raw));

                        match parsed {
                            Ok(pk) => {
                                let pk_hex = pk.to_string();
                                if app.contacts_manager.find_contact(&pk_hex).is_some() {
                                    app.state.contacts.add_error =
                                        Some("Contact already exists.".to_string());
                                } else {
                                    // Grab whatever metadata we already have cached
                                    let metadata = app
                                        .profile_metadata
                                        .get(&pk_hex)
                                        .and_then(|opt| match opt {
                                            ProfileOption::Some(m) => Some(m.clone()),
                                            _ => None,
                                        })
                                        .unwrap_or_default();

                                    if let Err(e) = app
                                        .contacts_manager
                                        .add_contact(&app.db, pk_hex, petname, metadata)
                                    {
                                        error!("Failed to add contact: {}", e);
                                        app.state.contacts.add_error =
                                            Some("Failed to add contact.".to_string());
                                    } else {
                                        // Reset form
                                        app.state.contacts.add_pubkey_input.clear();
                                        app.state.contacts.add_petname_input.clear();
                                        app.state.contacts.show_add_form = false;
                                        app.state.contacts.add_error = None;
                                    }
                                }
                            }
                            Err(_) => {
                                app.state.contacts.add_error = Some(
                                    "Invalid public key. Use npub1... or 64-char hex.".to_string(),
                                );
                            }
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

    // Track actions to apply after the loop (can't mutate app while iterating)
    let mut contact_to_remove: Option<String> = None;
    let mut petname_to_save: Option<(String, Option<String>)> = None;

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let total = app.contacts_manager.get_contacts().len();

            for index in 0..total {
                let contact = app.contacts_manager.get_contacts()[index].clone();
                app.contacts_manager.ensure_contact_images_loaded();

                let is_editing =
                    app.state.contacts.editing_pubkey.as_ref() == Some(&contact.pubkey);

                Frame::none()
                    .fill(style::CARD_BG)
                    .stroke(Stroke::new(1.0, style::CARD_STROKE))
                    .inner_margin(Margin::symmetric(16.0, 12.0))
                    .rounding(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            draw_contact_avatar(&app.contacts_manager, ui, &contact);
                            ui.add_space(12.0);

                            ui.vertical(|ui| {
                                // Petname row (editable)
                                if is_editing {
                                    ui.horizontal(|ui| {
                                        ui.label("Petname:");
                                        ui.text_edit_singleline(
                                            &mut app.state.contacts.editing_petname_buf,
                                        );
                                        if ui.button("Save").clicked() {
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
                                            petname_to_save =
                                                Some((contact.pubkey.clone(), petname));
                                            app.state.contacts.editing_pubkey = None;
                                        }
                                        if ui.button("Cancel").clicked() {
                                            app.state.contacts.editing_pubkey = None;
                                        }
                                    });
                                } else {
                                    let display = contact.display_name();
                                    ui.label(RichText::new(&display).strong());

                                    if let Some(petname) = &contact.petname {
                                        // Show the nostr name underneath the petname
                                        if let Some(nostr_name) = contact
                                            .metadata
                                            .display_name
                                            .as_ref()
                                            .or(contact.metadata.name.as_ref())
                                        {
                                            if nostr_name != petname {
                                                ui.label(
                                                    RichText::new(format!("({})", nostr_name))
                                                        .small()
                                                        .color(style::TEXT_MUTED),
                                                );
                                            }
                                        }
                                    } else if let Some(name) = contact.metadata.name.as_ref() {
                                        if contact.metadata.display_name.as_ref() != Some(name) {
                                            ui.label(name);
                                        }
                                    }

                                    ui.label(
                                        RichText::new(&contact.pubkey)
                                            .monospace()
                                            .small()
                                            .color(style::TEXT_MUTED),
                                    );
                                }
                            });

                            // Action buttons pushed to right
                            if !is_editing {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button(RichText::new("X").color(Color32::RED))
                                            .on_hover_text("Remove contact")
                                            .clicked()
                                        {
                                            contact_to_remove = Some(contact.pubkey.clone());
                                        }

                                        if ui.button("Edit").on_hover_text("Edit petname").clicked()
                                        {
                                            app.state.contacts.editing_pubkey =
                                                Some(contact.pubkey.clone());
                                            app.state.contacts.editing_petname_buf =
                                                contact.petname.clone().unwrap_or_default();
                                        }
                                    },
                                );
                            }
                        });
                    });

                ui.add_space(4.0);
            }
        });

    // Apply deferred mutations
    if let Some(pubkey) = contact_to_remove {
        if let Err(e) = app.contacts_manager.remove_contact(&app.db, &pubkey) {
            error!("Failed to remove contact: {}", e);
        }
    }
    if let Some((pubkey, petname)) = petname_to_save {
        if let Err(e) = app
            .contacts_manager
            .update_petname(&app.db, &pubkey, petname)
        {
            error!("Failed to update contact petname: {}", e);
        }
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
