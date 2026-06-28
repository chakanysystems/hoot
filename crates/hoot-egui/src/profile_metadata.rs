pub use hoot_backend::profile_metadata::ProfileOption;
pub use hoot_backend::ProfileMetadata;

use crate::Hoot;
use tracing::error;

pub fn get_profile_metadata(app: &mut Hoot, public_key: String) -> &ProfileOption {
    if !app.profile_metadata.contains_key(&public_key) {
        match app.backend.get_profile_metadata(public_key.clone()) {
            Ok(Some(metadata)) => {
                app.profile_metadata
                    .insert(public_key.clone(), ProfileOption::Some(metadata));
            }
            Ok(None) => {
                app.profile_metadata
                    .insert(public_key.clone(), ProfileOption::Waiting);
            }
            Err(e) => {
                error!("Couldn't fetch profile metadata: {}", e);
                app.profile_metadata
                    .insert(public_key.clone(), ProfileOption::Waiting);
            }
        }
    }
    app.profile_metadata
        .get(&public_key)
        .unwrap_or(&ProfileOption::Waiting)
}

pub fn update_logged_in_profile_metadata(
    app: &mut Hoot,
    public_key: String,
    metadata: ProfileMetadata,
) -> Result<(), String> {
    app.backend
        .update_profile_metadata(public_key.clone(), metadata.clone())
        .map_err(|e| e.to_string())?;
    app.profile_metadata
        .insert(public_key, ProfileOption::Some(metadata));
    Ok(())
}
