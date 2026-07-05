use hoot_backend::{Mailbox, SenderStatusDto};
use tracing::error;

use crate::Hoot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SenderDecision {
    Accept,
    Reject,
}

pub fn sender_decision_refresh_plan(decision: SenderDecision) -> [Mailbox; 2] {
    match decision {
        SenderDecision::Accept => [Mailbox::Requests, Mailbox::Inbox],
        SenderDecision::Reject => [Mailbox::Requests, Mailbox::Junk],
    }
}

impl Hoot {
    pub fn select_account(&mut self, pubkey: String) {
        match self.backend.set_active_account(Some(pubkey.clone())) {
            Ok(()) => self.active_account_pubkey = Some(pubkey),
            Err(e) => error!("Failed to select account: {}", e),
        }
    }

    pub fn refresh_mailbox(&mut self, mailbox: Mailbox) {
        match mailbox {
            Mailbox::Inbox => self.refresh_inbox(),
            Mailbox::Trash => self.refresh_trash(),
            Mailbox::Requests => self.refresh_requests(),
            Mailbox::Junk => self.refresh_junk(),
        }
    }

    pub fn apply_sender_decision(&mut self, pubkey: String, decision: SenderDecision) {
        let status = match decision {
            SenderDecision::Accept => SenderStatusDto::Allowed,
            SenderDecision::Reject => SenderStatusDto::Junked,
        };
        if let Err(e) = self.backend.set_sender_status(pubkey, status) {
            error!("Failed to update sender status: {}", e);
            return;
        }
        for mailbox in sender_decision_refresh_plan(decision) {
            self.refresh_mailbox(mailbox);
        }
    }

    pub fn remove_sender_status_and_refresh(&mut self, pubkey: String, mailboxes: &[Mailbox]) {
        if let Err(e) = self.backend.remove_sender_status(pubkey) {
            error!("Failed to remove sender status: {}", e);
            return;
        }
        for mailbox in mailboxes.iter().cloned() {
            self.refresh_mailbox(mailbox);
        }
    }

    pub fn delete_draft_and_refresh(&mut self, id: i64) {
        if let Err(e) = self.backend.delete_draft(id) {
            error!("Failed to delete draft: {}", e);
        }
        self.refresh_drafts();
    }

    pub fn restore_from_trash_and_refresh(&mut self, event_id: String) {
        if let Err(e) = self.backend.restore_from_trash(event_id) {
            error!("Failed to restore from trash: {}", e);
            return;
        }
        self.refresh_inbox();
        self.refresh_trash();
    }

    pub fn delete_messages_permanently_and_refresh(&mut self, event_ids: Vec<String>) {
        if let Err(e) = self.backend.delete_messages_permanently(event_ids) {
            error!("Failed to delete trashed event: {}", e);
            return;
        }
        self.refresh_trash();
    }

    pub fn delete_account_and_refresh(&mut self, pubkey: String) {
        if let Err(e) = self.backend.delete_account(pubkey) {
            error!("couldn't remove key: {}", e);
        }
        self.refresh_accounts();
    }

    pub fn add_relay_url(&mut self, url: String) {
        if let Err(e) = self.backend.add_relay(url) {
            error!("Failed to add relay: {}", e);
        }
    }

    pub fn remove_relay_url(&mut self, url: String) {
        if let Err(e) = self.backend.remove_relay(url) {
            error!("Failed to remove relay: {}", e);
        }
    }
}
