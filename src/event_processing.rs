use crate::mail_event::MAIL_EVENT_KIND;
use crate::profile_metadata::{ProfileMetadata, ProfileOption};
use crate::relay;
use crate::Hoot;
use crate::types::{HootStatus, Page};
use eframe::egui;
use nostr::event::Kind;
use nostr::TagKind;
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

pub fn try_recv_relay_message(app: &mut Hoot) {
    if let Some(raw) = app.relays.try_recv() {
        info!("{:?}", &raw);
        match relay::RelayMessage::from_json(&raw) {
            Ok(v) => process_message(app, &v),
            Err(e) => error!("could not decode message sent from relay: {}", e),
        }
    }
}

pub fn update_app(app: &mut Hoot, ctx: &egui::Context) {
    #[cfg(feature = "profiling")]
    puffin::profile_function!();
    let ctx = ctx.clone();
    let wake_ctx = ctx.clone();
    let wake_up = move || {
        wake_ctx.request_repaint();
    };

    if app.status == HootStatus::PreUnlock {
        info!("Requesting Database Unlock before proceeding.");
        app.status = HootStatus::WaitingForUnlock;
        let _ = app
            .relays
            .add_url("wss://relay.chakany.systems".to_string(), wake_up.clone());

        let _ = app
            .relays
            .add_url("wss://talon.quest".to_string(), wake_up.clone());

        app.relays.keepalive(wake_up);
        return;
    } else if app.status == HootStatus::WaitingForUnlock {
        // the unlock happens in the render_app function
        // we can't do anything but wait until HootStatus is Initializing
        app.relays.keepalive(wake_up);
        try_recv_relay_message(app);
        return;
    }

    if app.status == HootStatus::Initializing {
        info!("Initializing Hoot...");
        if let Err(e) = app.account_manager.load_keys(&app.db) {
            error!("something went wrong trying to load keys: {}", e);
        }

        if let Err(e) = app.db.purge_deleted_events() {
            error!("Failed to purge deleted events: {}", e);
        }

        let now = chrono::Utc::now().timestamp();
        if let Err(e) = app.db.purge_expired_trash(now) {
            error!("Failed to purge expired trash: {}", e);
        }

        match app.db.get_top_level_messages() {
            Ok(msgs) => app.table_entries = msgs,
            Err(e) => error!("Could not fetch table entries to display from DB: {}", e),
        }

        app.refresh_trash();

        if !app.account_manager.loaded_keys.is_empty() {
            app.update_gift_wrap_subscription();

            if let Err(e) = app
                .contacts_manager
                .load_from_db(&app.db, &mut app.profile_metadata)
            {
                error!("Failed to load contacts: {}", e);
            }
        }

        app.refresh_drafts();

        app.status = HootStatus::Ready;
        info!("Hoot Ready");
    }

    app.relays.keepalive(wake_up);
    try_recv_relay_message(app);
    app.contacts_manager.process_image_queue(&ctx);
}

fn process_message(app: &mut Hoot, msg: &relay::RelayMessage) {
    use relay::RelayMessage::*;
    match msg {
        Event(sub_id, event) => process_event(app, sub_id, event),
        Notice(msg) => debug!("Relay notice: {}", msg),
        OK(result) => debug!("Command result: {:?}", result),
        Eose(sub_id) => debug!("End of stored events for subscription {}", sub_id),
        Closed(sub_id, msg) => debug!("Subscription {} closed: {}", sub_id, msg),
    }
}

pub fn apply_deletions(
    app: &mut Hoot,
    event_ids: Vec<String>,
    author_pubkey: Option<&str>,
    source_event_id: Option<&str>,
) -> Result<(), anyhow::Error> {
    if event_ids.is_empty() {
        return Ok(());
    }

    let mut scoped_event_ids: Vec<String> = Vec::new();
    let mut unscoped_event_ids: Vec<String> = Vec::new();
    for event_id in event_ids {
        match app.db.get_event_kind_pubkey(&event_id) {
            Ok(Some((kind, pubkey))) => {
                let is_gift_wrap = kind == i64::from(Kind::GiftWrap.as_u16());
                let is_mail = kind == i64::from(MAIL_EVENT_KIND);
                if is_gift_wrap {
                    continue;
                }

                if is_mail {
                    if let Some(author) = author_pubkey {
                        if author == pubkey {
                            scoped_event_ids.push(event_id);
                        }
                    } else {
                        unscoped_event_ids.push(event_id);
                    }
                } else {
                    unscoped_event_ids.push(event_id);
                }
            }
            Ok(None) => {}
            Err(e) => error!("Failed to load event {} metadata: {}", event_id, e),
        }
    }

    let mut apply_event_ids: Vec<String> = Vec::new();
    apply_event_ids.extend(scoped_event_ids.iter().cloned());
    apply_event_ids.extend(unscoped_event_ids.iter().cloned());

    if apply_event_ids.is_empty() {
        return Ok(());
    }

    if !scoped_event_ids.is_empty() {
        app.db
            .record_deletions(&scoped_event_ids, author_pubkey, source_event_id)?;
    }
    if !unscoped_event_ids.is_empty() {
        app.db
            .record_deletions(&unscoped_event_ids, None, source_event_id)?;
    }

    if let Err(e) = app.db.delete_from_trash(&apply_event_ids) {
        error!("Failed to remove deleted events from trash: {}", e);
    }

    let mut wrap_ids: Vec<String> = Vec::new();
    for event_id in &apply_event_ids {
        match app.db.get_wrap_ids_for_inner(event_id) {
            Ok(ids) => wrap_ids.extend(ids),
            Err(e) => error!("Failed to load gift wrap ids for {}: {}", event_id, e),
        }
    }
    if !wrap_ids.is_empty() {
        app.db
            .record_deletion_markers(&wrap_ids, source_event_id)?;
    }

    let mut removed_ids: HashSet<String> = apply_event_ids.into_iter().collect();
    for wrap_id in wrap_ids {
        removed_ids.insert(wrap_id);
    }
    if !removed_ids.is_empty() {
        app.events
            .retain(|ev| !removed_ids.contains(&ev.id.to_string()));
        if removed_ids.contains(&app.focused_post) {
            app.page = Page::Inbox;
            app.focused_post.clear();
            app.show_trashed_post = false;
        }
        match app.db.get_top_level_messages() {
            Ok(msgs) => app.table_entries = msgs,
            Err(e) => error!("Could not fetch table entries to display from DB: {}", e),
        }
        app.refresh_trash();
    }
    Ok(())
}

fn process_event(app: &mut Hoot, _sub_id: &str, event_json: &str) {
    #[cfg(feature = "profiling")]
    puffin::profile_function!();

    let event = match serde_json::from_str::<nostr::Event>(event_json) {
        Ok(event) => event,
        Err(_) => {
            error!("Failed to parse event JSON: {}", event_json);
            return;
        }
    };

    if event.verify().is_err() {
        error!("Event verification failed for event: {}", event.id);
        return;
    }
    debug!("Verified event: {:?}", event);

    if event.kind == Kind::EventDeletion {
        let event_ids: Vec<String> = event.tags.event_ids().map(|id| id.to_hex()).collect();
        if !event_ids.is_empty() {
            let author_pubkey = event.pubkey.to_string();
            let deletion_id = event.id.to_string();
            if let Err(e) = apply_deletions(
                app,
                event_ids,
                Some(author_pubkey.as_str()),
                Some(deletion_id.as_str()),
            ) {
                error!("Failed to apply deletion event {}: {}", event.id, e);
            }
        }
        return;
    }

    let event_id = event.id.to_string();
    let event_author = event.pubkey.to_string();
    if let Ok(true) = app.db.is_deleted(&event_id, Some(event_author.as_str())) {
        debug!("Skipping deleted event: {}", event.id);
        return;
    }
    if let Ok(true) = app.db.is_trashed(&event_id) {
        debug!("Skipping trashed event: {}", event.id);
        return;
    }

    if event.kind == Kind::Metadata {
        debug!("Got profile metadata");

        let deserialized_metadata: ProfileMetadata =
            match serde_json::from_str(&event.content) {
                Ok(meta) => meta,
                Err(e) => {
                    error!("Invalid metadata event {}: {}", event.id, e);
                    return;
                }
            };
        app.profile_metadata.insert(
            event.pubkey.to_string(),
            ProfileOption::Some(deserialized_metadata.clone()),
        );
        app.contacts_manager
            .upsert_metadata(event.pubkey.to_string(), deserialized_metadata.clone());
        // TODO: evaluate perf cost of clone LOL
        match app.db.update_profile_metadata(event.clone()) {
            Ok(_) => { // wow who cares
            }
            Err(e) => error!("Error when saving profile metadata to DB: {}", e),
        }
        return;
    }

    if event.kind == Kind::GiftWrap {
        if let Ok(true) = app.db.gift_wrap_exists(&event.id.to_string()) {
            debug!("Skipping already stored gift wrap: {}", event.id);
            return;
        }
        if let Ok(true) = app.db.is_deleted(&event.id.to_string(), None) {
            debug!("Skipping deleted gift wrap: {}", event.id);
            return;
        }
        match app.account_manager.unwrap_gift_wrap(&event) {
            Ok(unwrapped) => {
                if unwrapped.sender != unwrapped.rumor.pubkey {
                    warn!("Gift wrap seal signer mismatch for event {}", event.id);
                    return;
                }

                let mut rumor = unwrapped.rumor.clone();
                rumor.ensure_id();
                if let Err(e) = rumor.verify_id() {
                    error!("Invalid rumor id for gift wrap {}: {}", event.id, e);
                    return;
                }
                let rumor_id = rumor
                    .id
                    .expect("Invalid Gift Wrapped Event: There is no ID!")
                    .to_hex();
                let author_pubkey = rumor.pubkey.to_string();
                if let Ok(true) = app.db.is_deleted(&rumor_id, Some(author_pubkey.as_str())) {
                    if let Err(e) = app.db.record_deletion_markers(
                        &[event.id.to_string()],
                        None,
                    ) {
                        error!("Failed to record gift wrap deletion {}: {}", event.id, e);
                    }
                    return;
                }
                if let Ok(true) = app.db.is_trashed(&rumor_id) {
                    let recipient = event
                        .tags
                        .find(TagKind::p())
                        .and_then(|tag| tag.content())
                        .map(|val| val.to_string());
                    if let Err(e) = app.db.save_gift_wrap_map(
                        &event.id.to_string(),
                        &rumor_id,
                        recipient.as_deref(),
                        event.created_at.as_u64() as i64,
                    ) {
                        error!("Failed to save gift wrap map for trashed rumor: {}", e);
                    }
                    return;
                }

                let recipient = event
                    .tags
                    .find(TagKind::p())
                    .and_then(|tag| tag.content())
                    .map(|val| val.to_string());

                app.events.push(event.clone());

                if let Err(e) = app
                    .db
                    .store_event(&event, Some(&unwrapped), recipient.as_deref())
                {
                    error!("Failed to store event in database: {}", e);
                } else {
                    debug!("Successfully stored event with id {} in database", event.id);
                }
            }
            Err(e) => {
                error!("Failed to unwrap gift wrap {}: {}", event.id, e);
            }
        }
        return;
    }

    if let Ok(true) = app.db.has_event(&event.id.to_string()) {
        debug!("Skipping already stored event: {}", event.id);
        return;
    }

    app.events.push(event.clone());

    if let Err(e) = app.db.store_event(&event, None, None) {
        error!("Failed to store event in database: {}", e);
    } else {
        debug!("Successfully stored event with id {} in database", event.id);
    }
}
