// so we need to implement a database and memory backed cache system for loading profile metadata.
// profile metadata will be stored inside of the sqlite database for non-volitle storage. When we need that data, we load it into a hashmap so it can be quickly accessed during
// our render time. The question I am putting forth to myself is this: is it worth it to load all of the data or to selectively load who we need? Maybe we can do a JOIN on startup
// fetching our messages and their profile metadata. When an unloaded comes in we simply fetch that too.
// Hmm that seems reasonable.

use crate::{
    keystorage::KeyStorage,
    relay::{Relay, Subscription},
    Hoot,
};
use anyhow::{Context, Result};
use nostr::PublicKey;
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
pub struct ProfileMetadata {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub picture: Option<String>,
}

/// This is our own little option type just for checking if we have a profile's
/// metadata within our own `Hoot::profile_metadata` struct's HashMap
/// Why? Because we may be looking for a profile's metadata, and need to comunicate that
/// so we know not to send another subscription.
///
/// This might not be the best approach, IDK we shall find out.
#[derive(Debug, PartialEq, Clone)]
pub enum ProfileOption {
    Waiting,
    Some(ProfileMetadata),
}

impl Default for ProfileOption {
    fn default() -> Self {
        ProfileOption::Waiting
    }
}

/// This creates a background job to fetch the profile metadata IF it isn't found.
/// here, id is the hex user public key. eventually I need to add a type for this
/// or use the nostr type. IDK.
pub fn get_profile_metadata(app: &mut Hoot, public_key: String) -> &ProfileOption {
    if !app.profile_metadata.contains_key(&public_key) {
        // check if db has what we want
        let db_metadata_opt = match app.db.get_profile_metadata(&public_key) {
            Ok(v) => v,
            Err(e) => {
                error!("Couldn't fetch profile metadata from database: {}", e);
                None
            }
        };

        let mut sub = Subscription::default();
        use std::str::FromStr;
        let filter = nostr::Filter::new()
            .kind(nostr::Kind::Metadata)
            .author(PublicKey::from_str(&public_key).unwrap());

        sub.filter(filter);

        let _ = app.relays.add_subscription(sub);
        // Tell that we are waiting for the metadata to come in.
        if let Some(meta) = db_metadata_opt {
            let val = ProfileOption::Some(meta);
            app.profile_metadata.insert(public_key.clone(), val);
            return app
                .profile_metadata
                .get(&public_key)
                .unwrap_or(&ProfileOption::Waiting);
        }
        app.profile_metadata
            .insert(public_key, ProfileOption::Waiting);
        return &ProfileOption::Waiting;
    }
    return app
        .profile_metadata
        .get(&public_key)
        .unwrap_or(&ProfileOption::Waiting);
}

/// Only for the profile metadata of logged in accounts.
pub fn update_logged_in_profile_metadata(
    app: &mut Hoot,
    public_key: PublicKey,
    metadata: ProfileMetadata,
) -> Result<()> {
    // update our in-memory representation
    app.profile_metadata.insert(
        public_key.to_string(),
        ProfileOption::Some(metadata.to_owned()),
    );
    app.upsert_contact(public_key.to_string(), metadata.clone());

    // convert into nostr event
    let serialized = serde_json::to_string(&metadata)?;
    let our_key = app
        .account_manager
        .loaded_keys
        .iter()
        .find(|v| v.public_key() == public_key)
        .context("Could not update our own account's metadata because we can't find the keys.")?;
    let event =
        nostr::EventBuilder::new(nostr::Kind::Metadata, serialized).sign_with_keys(our_key)?;

    // write to db
    // TODO: serializing and then deserialzing is retarded. fix.
    app.db.write_profile_metadata(event.clone())?;

    // send over wire
    // man i need to improve these ergonomics
    app.relays
        .send(ewebsock::WsMessage::Text(serde_json::to_string(
            &crate::relay::ClientMessage::Event { event },
        )?)).unwrap();

    Ok(())
}
