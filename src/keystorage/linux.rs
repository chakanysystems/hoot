#![cfg(target_os = "linux")]
use super::{Error, KeyStorage};
use nostr::Keys;

pub struct LinuxKeyStorage {}

impl LinuxKeyStorage {
    pub fn new() -> Self {
        Self {}
    }
}

impl KeyStorage for LinuxKeyStorage {
    fn get_keys(&self) -> Result<Vec<Keys>, Error> {
        let bfs = BasicFileStorage::new().get_keys()?;
        Ok(bfs)
    }
    fn add_key(&self, key: &Keys) -> Result<(), Error> {
        BasicFileStorage::new().add_key(key)?;
        Ok(())
    }
    fn remove_key(&self, key: &Keys) -> Result<(), Error> {
        BasicFileStorage::new().remove_key(key)?;
        Ok(())
    }
}

struct BasicFileStorage {
    credentials_dir: String,
}

impl BasicFileStorage {
    pub fn new() -> Self {
        BasicFileStorage {
            credentials_dir: ".credentials".to_string(),
        }
    }

    fn write_private_key(&self, keypair: &Keys) -> Result<(), Error> {
        use std::fs::{self, File};
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;

        fs::create_dir_all(&self.credentials_dir)?;

        let public_key = keypair.public_key().to_hex();
        let private_key = keypair.secret_key().to_secret_hex();

        let file_path = Path::new(&self.credentials_dir).join(&public_key);

        let mut file = File::create(&file_path)?;
        file.write_all(private_key.as_bytes())?;

        let mut perms = file.metadata()?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&file_path, perms)?;

        Ok(())
    }

    fn read_private_keys(&self) -> Result<Vec<Keys>, Error> {
        use std::fs::{self, File};
        use std::io::Read;
        use std::path::Path;

        let mut keypairs: Vec<Keys> = Vec::new();
        let dir_path = Path::new(&self.credentials_dir);

        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let mut file = File::open(&path)?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;

                if let Ok(keypair) = Keys::parse(contents) {
                    keypairs.push(keypair);
                }
            }
        }

        Ok(keypairs)
    }

    fn remove_keypair(&self, keypair: &Keys) -> Result<(), Error> {
        use std::fs;
        use std::path::Path;

        let public_key = keypair.public_key().to_string();
        let file_path = Path::new(&self.credentials_dir).join(&public_key);

        if file_path.exists() {
            fs::remove_file(file_path)?;
            Ok(())
        } else {
            Err(Error::IOError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Key file not found",
            )))
        }
    }
}

impl KeyStorage for BasicFileStorage {
    fn get_keys(&self) -> Result<Vec<Keys>, Error> {
        let mmt = self.read_private_keys()?;
        Ok(mmt)
    }
    fn add_key(&self, key: &Keys) -> Result<(), Error> {
        self.write_private_key(key)?;
        Ok(())
    }
    fn remove_key(&self, key: &Keys) -> Result<(), Error> {
        self.remove_keypair(key)?;
        Ok(())
    }
}
