use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use std::thread;
use tracing::{debug, warn};

pub struct ImageMessage {
    pub key: String,
    pub image: Option<ColorImage>,
}

pub struct ImageLoader {
    images: HashMap<String, TextureHandle>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    sender: Sender<ImageMessage>,
    receiver: Receiver<ImageMessage>,
}

impl ImageLoader {
    pub fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            images: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            sender,
            receiver,
        }
    }

    pub fn request(&mut self, key: String, url: String) {
        // Skip if already loaded, pending, or failed
        if self.images.contains_key(&key)
            || self.pending.contains(&key)
            || self.failed.contains(&key)
        {
            return;
        }

        let sender = self.sender.clone();
        let key_clone = key.clone();

        self.pending.insert(key);

        thread::spawn(move || {
            let image = fetch_image(&url);
            if sender.send(ImageMessage { key: key_clone, image }).is_err() {
                debug!("Image receiver dropped before image arrived");
            }
        });
    }

    pub fn process_queue(&mut self, ctx: &egui::Context) -> bool {
        let mut updated = false;

        while let Ok(message) = self.receiver.try_recv() {
            self.pending.remove(&message.key);

            if let Some(image) = message.image {
                let texture = ctx.load_texture(
                    format!("image-{}", message.key),
                    image,
                    TextureOptions::LINEAR,
                );
                self.images.insert(message.key, texture);
            } else {
                self.failed.insert(message.key);
            }

            updated = true;
        }

        if updated {
            ctx.request_repaint();
        }

        updated
    }

    pub fn get_texture(&self, key: &str) -> Option<&TextureHandle> {
        self.images.get(key)
    }

    pub fn invalidate(&mut self, key: &str) {
        self.images.remove(key);
        self.pending.remove(key);
        self.failed.remove(key);
    }
}

fn fetch_image(url: &str) -> Option<ColorImage> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        debug!("Skipping unsupported image URL: {}", url);
        return None;
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            debug!("Failed to build HTTP client for image: {}", err);
            return None;
        }
    };

    match client.get(url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                warn!(
                    "Image request returned status {} for {}",
                    response.status(),
                    url
                );
                return None;
            }

            match response.bytes() {
                Ok(bytes) => decode_image(bytes.as_ref()),
                Err(err) => {
                    debug!("Failed to read image bytes: {}", err);
                    None
                }
            }
        }
        Err(err) => {
            debug!("Failed to fetch image {}: {}", url, err);
            None
        }
    }
}

fn decode_image(bytes: &[u8]) -> Option<ColorImage> {
    let mut rgba = match image::load_from_memory(bytes) {
        Ok(img) => img.to_rgba8(),
        Err(err) => {
            debug!("Failed to decode image: {}", err);
            return None;
        }
    };

    if rgba.width() > 256 || rgba.height() > 256 {
        rgba = image::imageops::resize(&rgba, 256, 256, image::imageops::FilterType::Triangle);
    }

    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = rgba
        .as_raw()
        .chunks_exact(4)
        .map(|chunk| eframe::egui::Color32::from_rgba_unmultiplied(chunk[0], chunk[1], chunk[2], chunk[3]))
        .collect::<Vec<_>>();

    Some(ColorImage { size, pixels })
}
