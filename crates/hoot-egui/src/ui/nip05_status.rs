use eframe::egui::Color32;
use hoot_backend::Nip05Entry;

pub fn status_display(entry: &Nip05Entry) -> (&'static str, Color32, &'static str) {
    if entry.last_verified.is_some() {
        ("✓", Color32::GREEN, "Verified NIP-05")
    } else if entry.last_checked.is_some() {
        ("✗", Color32::RED, "Unverified NIP-05 - verification failed")
    } else {
        ("?", Color32::GRAY, "NIP-05 verification pending")
    }
}
