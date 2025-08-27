// so we need to implement a database and memory backed cache system for loading profile metadata.
// profile metadata will be stored inside of the sqlite database for non-volitle storage. When we need that data, we load it into a hashmap so it can be quickly accessed during
// our render time. The question I am putting forth to myself is this: is it worth it to load all of the data or to selectively load who we need? Maybe we can do a JOIN on startup
// fetching our messages and their profile metadata. When an unloaded comes in we simply fetch that too.
// Hmm that seems reasonable.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
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
pub enum ProfileOption {
    Waiting,
    Some(ProfileMetadata),
}
