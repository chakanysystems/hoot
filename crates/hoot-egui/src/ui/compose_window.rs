use crate::style;
use eframe::egui::{self, Color32, CornerRadius, FontId, RichText, Stroke};
use hoot_backend::{ComposeMessageInput, DraftDto, DraftInput, ParsedRecipient};
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nip05Resolution {
    Pending,
    Resolved(String),
    Failed,
}

#[derive(Debug, Clone)]
pub struct Recipient {
    pub raw: String,
    pub kind: RecipientKind,
}

#[derive(Debug, Clone)]
pub enum RecipientKind {
    Pubkey(String),
    Nip05 {
        identifier: String,
        resolution: Nip05Resolution,
    },
}

pub fn parse_recipient_token(token: &str) -> Option<RecipientKind> {
    hoot_backend::parse_recipient_token(token).map(|recipient| match recipient {
        ParsedRecipient::Pubkey(pubkey) => RecipientKind::Pubkey(pubkey),
        ParsedRecipient::Nip05 { identifier } => RecipientKind::Nip05 {
            identifier,
            resolution: Nip05Resolution::Pending,
        },
    })
}

pub fn serialize_recipients(recipients: &[Recipient]) -> String {
    recipients
        .iter()
        .map(|r| r.raw.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone)]
pub struct ComposeWindowState {
    pub subject: String,
    pub to_input: String,
    pub recipients: Vec<Recipient>,
    pub parent_event_ids: Vec<String>,
    pub content: String,
    pub selected_account_pubkey: Option<String>,
    pub selected_nip05: Option<String>,
    pub minimized: bool,
    pub draft_id: Option<i64>,
    pub send_status: Option<(String, Color32)>,
}

enum DraftAction {
    None,
    Save(DraftInput, Option<i64>),
    Delete(i64),
}

pub struct ComposeWindow {}

#[derive(Debug, Default, Clone, Copy)]
pub struct ComposePanelOutput {
    pub close_clicked: bool,
    pub sent_message: bool,
}

fn consume_pending_recipient_input(
    state: &mut ComposeWindowState,
) -> Result<Option<Recipient>, String> {
    let pending_token = state
        .to_input
        .trim_end_matches([' ', ','])
        .trim()
        .to_string();
    if pending_token.len() != state.to_input.len() {
        state.to_input = pending_token.clone();
    }
    if pending_token.is_empty() {
        return Ok(None);
    }

    match parse_recipient_token(&pending_token) {
        Some(kind) => {
            let raw = match &kind {
                RecipientKind::Nip05 { identifier, .. } => identifier.clone(),
                RecipientKind::Pubkey(_) => pending_token,
            };
            state.to_input.clear();
            Ok(Some(Recipient { raw, kind }))
        }
        None => Err(pending_token),
    }
}

fn flush_pending_recipient_input(app: &mut crate::Hoot, state: &mut ComposeWindowState) -> bool {
    match consume_pending_recipient_input(state) {
        Ok(Some(recipient)) => {
            if let RecipientKind::Nip05 { identifier, .. } = &recipient.kind {
                let _ = app.backend.request_nip05_resolution(identifier.clone());
            }
            state.recipients.push(recipient);
            true
        }
        Ok(None) => true,
        Err(invalid_token) => {
            state.send_status = Some((
                format!("Invalid recipient: {}", invalid_token),
                Color32::RED,
            ));
            false
        }
    }
}

fn mark_failed_nip05(state: &mut ComposeWindowState, failed: &[String]) {
    for recipient in &mut state.recipients {
        if let RecipientKind::Nip05 {
            identifier,
            resolution,
        } = &mut recipient.kind
        {
            if failed.iter().any(|value| value == identifier) {
                *resolution = Nip05Resolution::Failed;
            }
        }
    }
}

fn apply_nip05_resolution_to_recipients(
    state: &mut ComposeWindowState,
    nip05: &str,
    backend_resolution: &hoot_backend::Nip05ResolutionDto,
) {
    for recipient in &mut state.recipients {
        if let RecipientKind::Nip05 {
            identifier,
            resolution,
        } = &mut recipient.kind
        {
            if identifier == nip05 {
                *resolution = match &backend_resolution.status {
                    hoot_backend::Nip05ResolutionStatusDto::Pending => Nip05Resolution::Pending,
                    hoot_backend::Nip05ResolutionStatusDto::Resolved => backend_resolution
                        .pubkey_hex
                        .clone()
                        .map(Nip05Resolution::Resolved)
                        .unwrap_or(Nip05Resolution::Failed),
                    hoot_backend::Nip05ResolutionStatusDto::Failed => Nip05Resolution::Failed,
                };
            }
        }
    }
}

fn sync_nip05_resolutions(app: &mut crate::Hoot, state: &mut ComposeWindowState) {
    let pending_nip05: Vec<String> = state
        .recipients
        .iter()
        .filter_map(|recipient| match &recipient.kind {
            RecipientKind::Nip05 {
                identifier,
                resolution: Nip05Resolution::Pending,
            } => Some(identifier.clone()),
            _ => None,
        })
        .collect();

    for identifier in pending_nip05 {
        match app.backend.get_nip05_resolution(identifier.clone()) {
            Ok(Some(resolution)) => {
                apply_nip05_resolution_to_recipients(state, &identifier, &resolution);
            }
            Ok(None) => {}
            Err(e) => error!("Failed to get NIP-05 resolution: {}", e),
        }
    }
}

fn send_message(app: &mut crate::Hoot, state: &mut ComposeWindowState) -> ComposePanelOutput {
    if !flush_pending_recipient_input(app, state) {
        return ComposePanelOutput::default();
    }

    let to_field = serialize_recipients(&state.recipients);
    if to_field.trim().is_empty() {
        state.send_status = Some(("No valid recipients".to_string(), Color32::RED));
        return ComposePanelOutput::default();
    }

    let input = ComposeMessageInput {
        subject: state.subject.clone(),
        content: state.content.clone(),
        to_field,
        parent_event_ids: state.parent_event_ids.clone(),
        selected_account_pubkey: state
            .selected_account_pubkey
            .clone()
            .or_else(|| app.active_account_pubkey.clone()),
        selected_nip05: state.selected_nip05.clone(),
    };

    match app.backend.send_message(input) {
        Ok(result) if !result.pending_nip05.is_empty() => {
            state.send_status = Some(("Resolving NIP-05 addresses...".to_string(), style::TEXT2));
            ComposePanelOutput::default()
        }
        Ok(result) if !result.failed_nip05.is_empty() => {
            mark_failed_nip05(state, &result.failed_nip05);
            state.send_status = Some((
                format!("Could not resolve: {}", result.failed_nip05.join(", ")),
                Color32::RED,
            ));
            ComposePanelOutput::default()
        }
        Ok(result) => {
            info!("Sent {} message event(s)", result.sent_count);
            state.send_status = Some(("Message sent".to_string(), style::GREEN));
            ComposePanelOutput {
                close_clicked: false,
                sent_message: true,
            }
        }
        Err(e) => {
            error!("Failed to send message: {}", e);
            state.send_status = Some((format!("Failed to send: {}", e), Color32::RED));
            ComposePanelOutput::default()
        }
    }
}

fn save_draft(state: &mut ComposeWindowState) -> DraftAction {
    DraftAction::Save(
        DraftInput {
            subject: state.subject.clone(),
            to_field: serialize_recipients(&state.recipients),
            content: state.content.clone(),
            parent_events: state.parent_event_ids.clone(),
            selected_account: state.selected_account_pubkey.clone(),
            selected_nip05: state.selected_nip05.clone(),
        },
        state.draft_id,
    )
}

fn apply_draft_action(
    app: &mut crate::Hoot,
    state: &mut ComposeWindowState,
    draft_action: DraftAction,
) {
    match draft_action {
        DraftAction::Save(input, Some(id_existing)) => {
            let draft = DraftDto {
                id: id_existing,
                subject: input.subject,
                to_field: input.to_field,
                content: input.content,
                parent_events: input.parent_events,
                selected_account: input.selected_account,
                selected_nip05: input.selected_nip05,
                created_at: 0,
                updated_at: 0,
            };
            match app.backend.update_draft(draft) {
                Ok(()) => info!("Draft updated"),
                Err(e) => error!("Failed to update draft: {}", e),
            }
            app.refresh_drafts();
        }
        DraftAction::Save(input, None) => {
            match app.backend.save_draft(input) {
                Ok(new_id) => {
                    state.draft_id = Some(new_id);
                    info!("Draft saved with id {}", new_id);
                }
                Err(e) => error!("Failed to save draft: {}", e),
            }
            app.refresh_drafts();
        }
        DraftAction::Delete(draft_id) => {
            match app.backend.delete_draft(draft_id) {
                Ok(()) => state.draft_id = None,
                Err(e) => error!("Failed to delete draft after send: {}", e),
            }
            app.refresh_drafts();
        }
        DraftAction::None => {}
    }
}

pub fn hydrate_recipients(to_field: &str) -> Vec<Recipient> {
    to_field
        .split_whitespace()
        .filter_map(|token| {
            parse_recipient_token(token).map(|kind| Recipient {
                raw: match &kind {
                    RecipientKind::Nip05 { identifier, .. } => identifier.clone(),
                    RecipientKind::Pubkey(_) => token.to_string(),
                },
                kind,
            })
        })
        .collect()
}

pub fn render_panel(
    app: &mut crate::Hoot,
    ui: &mut egui::Ui,
    id: egui::Id,
    title: &str,
    show_close_button: bool,
    request_body_focus: bool,
    state: &mut ComposeWindowState,
) -> ComposePanelOutput {
    if state.selected_account_pubkey.is_none() {
        state.selected_account_pubkey = app.active_account_pubkey.clone().or_else(|| {
            app.accounts
                .first()
                .map(|account| account.pubkey_hex.clone())
        });
    }

    sync_nip05_resolutions(app, state);

    let mut output = ComposePanelOutput::default();
    let mut draft_action = DraftAction::None;
    let header_resp = egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 16,
            right: 12,
            top: 12,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if show_close_button {
                    if style::pointer(
                        ui.add(
                            egui::Button::new(RichText::new("x").size(13.0).color(style::TEXT2))
                                .fill(style::SURFACE2)
                                .stroke(Stroke::NONE)
                                .corner_radius(CornerRadius::same(6))
                                .min_size(egui::vec2(24.0, 24.0)),
                        ),
                    )
                    .clicked()
                    {
                        output.close_clicked = true;
                    }
                    ui.add_space(6.0);
                }

                ui.label(RichText::new(title).size(14.0).color(style::TEXT).strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let to_empty = state.recipients.is_empty() && state.to_input.trim().is_empty();
                    let send_btn = egui::Button::new(
                        RichText::new("Send")
                            .color(Color32::WHITE)
                            .size(13.0)
                            .strong(),
                    )
                    .fill(if to_empty {
                        style::TEXT3
                    } else {
                        style::ACCENT
                    })
                    .corner_radius(CornerRadius::same(8))
                    .min_size(egui::vec2(64.0, 28.0));

                    if style::pointer(ui.add_enabled(!to_empty, send_btn)).clicked() {
                        let send_output = send_message(app, state);
                        output.sent_message = send_output.sent_message;
                        if send_output.sent_message {
                            if let Some(draft_id) = state.draft_id {
                                draft_action = DraftAction::Delete(draft_id);
                            }
                        }
                    }
                });
            });
        });

    let header_rect = header_resp.response.rect;
    ui.painter().line_segment(
        [
            egui::pos2(header_rect.left(), header_rect.bottom()),
            egui::pos2(header_rect.right(), header_rect.bottom()),
        ],
        Stroke::new(1.0, style::border()),
    );

    let footer_height = 44.0;
    let available_for_body = (ui.available_height() - footer_height).max(120.0);

    egui::ScrollArea::vertical()
        .id_salt(id.with("fields_scroll"))
        .max_height(available_for_body)
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 16,
                    right: 16,
                    top: 10,
                    bottom: 4,
                })
                .show(ui, |ui| {
                    let mut new_recipient: Option<Recipient> = None;
                    let mut remove_idx: Option<usize> = None;
                    let mut nip05_to_request: Option<String> = None;
                    let mut to_input_focused = false;
                    let mut to_input_rect = egui::Rect::NOTHING;
                    let mut to_underline_left = 0f32;

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("TO").size(11.0).color(style::TEXT3).strong());
                        ui.add_space(4.0);
                        to_underline_left = ui.cursor().left();

                        for (i, recipient) in state.recipients.iter().enumerate() {
                            let (label_text, label_color) = match &recipient.kind {
                                RecipientKind::Pubkey(_) => {
                                    let raw = &recipient.raw;
                                    let char_count = raw.chars().count();
                                    let truncated = if char_count > 20 {
                                        let head: String = raw.chars().take(8).collect();
                                        let tail: String = raw
                                            .chars()
                                            .rev()
                                            .take(4)
                                            .collect::<String>()
                                            .chars()
                                            .rev()
                                            .collect();
                                        format!("{}…{}", head, tail)
                                    } else {
                                        raw.clone()
                                    };
                                    (truncated, style::TEXT)
                                }
                                RecipientKind::Nip05 {
                                    identifier,
                                    resolution,
                                } => {
                                    let (icon, color) = match resolution {
                                        Nip05Resolution::Pending => ("? ", style::TEXT3),
                                        Nip05Resolution::Resolved(_) => ("✓ ", style::GREEN),
                                        Nip05Resolution::Failed => ("✗ ", Color32::RED),
                                    };
                                    (format!("{}{}", icon, identifier), color)
                                }
                            };

                            egui::Frame::new()
                                .fill(style::SURFACE2)
                                .stroke(egui::Stroke::new(1.0, style::border()))
                                .corner_radius(egui::CornerRadius::same(6))
                                .inner_margin(egui::Margin {
                                    left: 6,
                                    right: 4,
                                    top: 2,
                                    bottom: 2,
                                })
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        ui.label(
                                            RichText::new(&label_text)
                                                .size(12.5)
                                                .color(label_color),
                                        );
                                        if style::pointer(
                                            ui.add(
                                                egui::Button::new(
                                                    RichText::new("×")
                                                        .size(12.0)
                                                        .color(style::TEXT3),
                                                )
                                                .frame(false)
                                                .min_size(egui::vec2(14.0, 14.0)),
                                            ),
                                        )
                                        .clicked()
                                        {
                                            remove_idx = Some(i);
                                        }
                                    });
                                });
                        }

                        let input_width =
                            (ui.available_width() - ui.spacing().item_spacing.x).max(120.0);
                        let input_resp = ui.add(
                            egui::TextEdit::singleline(&mut state.to_input)
                                .hint_text(if state.recipients.is_empty() {
                                    "npub, hex, or user@domain.com"
                                } else {
                                    ""
                                })
                                .frame(false)
                                .desired_width(input_width)
                                .font(egui::FontId::proportional(13.5))
                                .text_color(style::TEXT)
                                .lock_focus(true)
                                .id_salt(id.with("to_input")),
                        );

                        let commit_keyed = input_resp.has_focus()
                            && ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            });
                        let commit_delim =
                            state.to_input.ends_with(' ') || state.to_input.ends_with(',');

                        to_input_focused = input_resp.has_focus();
                        to_input_rect = input_resp.rect;

                        if commit_keyed || commit_delim {
                            match consume_pending_recipient_input(state) {
                                Ok(Some(recipient)) => {
                                    if let RecipientKind::Nip05 { identifier, .. } = &recipient.kind
                                    {
                                        nip05_to_request = Some(identifier.clone());
                                    }
                                    new_recipient = Some(recipient);
                                }
                                Ok(None) => {}
                                Err(_) => {}
                            }
                        }
                    });

                    let y = to_input_rect.bottom() + 3.0;
                    let (line_color, line_width) = if to_input_focused {
                        (style::ACCENT, 2.0)
                    } else {
                        (style::border_strong(), 1.0)
                    };
                    ui.painter().line_segment(
                        [
                            egui::pos2(to_underline_left, y),
                            egui::pos2(to_input_rect.right(), y),
                        ],
                        egui::Stroke::new(line_width, line_color),
                    );

                    if let Some(idx) = remove_idx {
                        state.recipients.remove(idx);
                    }
                    if let Some(recipient) = new_recipient {
                        state.recipients.push(recipient);
                    }
                    if let Some(nip05) = nip05_to_request {
                        let _ = app.backend.request_nip05_resolution(nip05);
                    }
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("SUBJECT")
                                .size(11.0)
                                .color(style::TEXT3)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        style::underline_text_edit(ui, &mut state.subject, "Subject", 13.5);
                    });

                    ui.add_space(16.0);

                    let body_response = ui.add(
                        egui::TextEdit::multiline(&mut state.content)
                            .hint_text("Write your message…")
                            .frame(false)
                            .desired_rows(12)
                            .desired_width(f32::INFINITY)
                            .font(FontId::proportional(14.0))
                            .text_color(style::TEXT)
                            .id_salt(id.with("body_input")),
                    );
                    if request_body_focus {
                        body_response.request_focus();
                    }
                });
        });

    let footer_top_y = ui.cursor().top();
    ui.painter().line_segment(
        [
            egui::pos2(ui.max_rect().left(), footer_top_y),
            egui::pos2(ui.max_rect().right(), footer_top_y),
        ],
        Stroke::new(1.0, style::border()),
    );

    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 6,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let selected_text = state
                    .selected_account_pubkey
                    .as_ref()
                    .and_then(|pubkey| {
                        app.accounts
                            .iter()
                            .find(|account| &account.pubkey_hex == pubkey)
                    })
                    .map(account_label)
                    .unwrap_or_else(|| "Select account".to_string());

                let account_selector = egui::ComboBox::from_id_salt(id.with("account_selector"))
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for account in app.accounts.clone() {
                            let selected = state.selected_account_pubkey.as_deref()
                                == Some(account.pubkey_hex.as_str());

                            let display_text = format!("{} (raw key)", account_label(&account));
                            if style::pointer(ui.selectable_label(
                                selected && state.selected_nip05.is_none(),
                                &display_text,
                            ))
                            .clicked()
                            {
                                state.selected_account_pubkey = Some(account.pubkey_hex.clone());
                                state.selected_nip05 = None;
                            }

                            if let Ok(nip05s) = app
                                .backend
                                .get_nip05s_for_pubkey(account.pubkey_hex.clone())
                            {
                                for nip05_entry in nip05s {
                                    let nip05_selected = selected
                                        && state.selected_nip05.as_ref()
                                            == Some(&nip05_entry.nip05);
                                    let (status_icon, _, _) =
                                        crate::ui::nip05_status::status_display(&nip05_entry);
                                    let display_text = format!(
                                        "{} {} ({})",
                                        status_icon,
                                        nip05_entry.nip05,
                                        account_label(&account)
                                    );
                                    if style::pointer(
                                        ui.selectable_label(nip05_selected, &display_text),
                                    )
                                    .clicked()
                                    {
                                        state.selected_account_pubkey =
                                            Some(account.pubkey_hex.clone());
                                        state.selected_nip05 = Some(nip05_entry.nip05);
                                    }
                                }
                            }
                        }
                    });
                let _ = style::pointer(account_selector.response);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if style::pointer(
                        ui.add(
                            egui::Button::new(RichText::new("Save Draft").size(13.0))
                                .corner_radius(CornerRadius::same(6)),
                        ),
                    )
                    .clicked()
                    {
                        if flush_pending_recipient_input(app, state) {
                            draft_action = save_draft(state);
                            state.send_status = Some(("Draft saved".to_string(), style::TEXT2));
                        }
                    }

                    if let Some((msg, color)) = &state.send_status {
                        ui.label(RichText::new(msg).color(*color).small());
                    }
                });
            });
        });

    apply_draft_action(app, state, draft_action);

    output
}

fn account_label(account: &hoot_backend::AccountSummary) -> String {
    if let Some(name) = &account.display_name {
        if !name.is_empty() {
            return name.clone();
        }
    }
    if !account.npub.is_empty() {
        if account.npub.len() > 16 {
            format!("{}...", &account.npub[..16])
        } else {
            account.npub.clone()
        }
    } else {
        account.pubkey_hex.clone()
    }
}

impl ComposeWindow {
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) -> bool {
        let screen_rect = ctx.content_rect();
        let mut state = app
            .state
            .compose_window
            .remove(&id)
            .expect("no state found for id");
        let mut output = ComposePanelOutput::default();

        egui::Window::new("compose_window")
            .id(id)
            .title_bar(false)
            .default_size([600.0, 420.0])
            .min_width(380.0)
            .min_height(260.0)
            .default_pos([screen_rect.right() - 620.0, screen_rect.bottom() - 440.0])
            .frame(
                egui::Frame::new()
                    .fill(style::SURFACE)
                    .stroke(Stroke::new(1.0, style::border_strong()))
                    .corner_radius(CornerRadius::same(12))
                    .shadow(style::shadow_lg()),
            )
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                output = render_panel(
                    app,
                    ui,
                    id.with("panel"),
                    "New Message",
                    true,
                    false,
                    &mut state,
                );
            });

        if !output.close_clicked {
            app.state.compose_window.insert(id, state);
        }

        !output.close_clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_state_with_input(to_input: &str) -> ComposeWindowState {
        ComposeWindowState {
            subject: String::new(),
            to_input: to_input.to_string(),
            recipients: Vec::new(),
            parent_event_ids: Vec::new(),
            content: String::new(),
            selected_account_pubkey: None,
            selected_nip05: None,
            minimized: false,
            draft_id: None,
            send_status: None,
        }
    }

    #[test]
    fn compose_recipient_parser_uses_backend_contract() {
        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        assert!(matches!(
            hoot_backend::parse_recipient_token(npub),
            Some(hoot_backend::ParsedRecipient::Pubkey(_))
        ));

        let hex = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefaf0f1";
        assert!(matches!(
            hoot_backend::parse_recipient_token(hex),
            Some(hoot_backend::ParsedRecipient::Pubkey(parsed_hex)) if parsed_hex == hex
        ));

        assert!(matches!(
            hoot_backend::parse_recipient_token("Bob@Example.COM"),
            Some(hoot_backend::ParsedRecipient::Nip05 { identifier }) if identifier == "bob@example.com"
        ));

        for token in ["", "   ", "notakey", "bob@@example.com", "@example.com"] {
            assert!(
                hoot_backend::parse_recipient_token(token).is_none(),
                "expected {token:?} to be rejected by backend recipient parser"
            );
        }
    }

    #[test]
    fn compose_updates_nip05_chip_when_backend_resolution_completes() {
        let mut state = empty_state_with_input("");
        state.recipients.push(Recipient {
            raw: "jack@chakany.systems".to_string(),
            kind: RecipientKind::Nip05 {
                identifier: "jack@chakany.systems".to_string(),
                resolution: Nip05Resolution::Pending,
            },
        });

        apply_nip05_resolution_to_recipients(
            &mut state,
            "jack@chakany.systems",
            &hoot_backend::Nip05ResolutionDto {
                status: hoot_backend::Nip05ResolutionStatusDto::Resolved,
                pubkey_hex: Some(
                    "c5fb6ecc876e0458e3eca9918e370cbcd376901c58460512fe537a46e58c38bb".to_string(),
                ),
            },
        );

        match &state.recipients[0].kind {
            RecipientKind::Nip05 { resolution, .. } => {
                assert!(matches!(resolution, Nip05Resolution::Resolved(pubkey)
                    if pubkey == "c5fb6ecc876e0458e3eca9918e370cbcd376901c58460512fe537a46e58c38bb"));
            }
            other => panic!("expected NIP-05 recipient, got {other:?}"),
        }
    }

    #[test]
    fn test_serialize_recipients_roundtrip() {
        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        let recipients = vec![
            Recipient {
                raw: npub.to_string(),
                kind: parse_recipient_token(npub).unwrap(),
            },
            Recipient {
                raw: "alice@example.com".to_string(),
                kind: parse_recipient_token("alice@example.com").unwrap(),
            },
        ];
        let serialized = serialize_recipients(&recipients);
        let tokens: Vec<&str> = serialized.split_whitespace().collect();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], npub);
        assert_eq!(tokens[1], "alice@example.com");
    }

    #[test]
    fn test_consume_pending_recipient_accepts_trailing_delimiter() {
        let mut state = empty_state_with_input("bob@example.com,");
        let result = consume_pending_recipient_input(&mut state);
        assert!(matches!(result, Ok(Some(Recipient { .. }))));
        assert!(state.to_input.is_empty());
    }

    #[test]
    fn test_consume_pending_recipient_preserves_invalid_token() {
        let mut state = empty_state_with_input("notakey,");
        let result = consume_pending_recipient_input(&mut state);
        assert_eq!(result.unwrap_err(), "notakey");
        assert_eq!(state.to_input, "notakey");
    }
}
