use nostr::Keys;

mod linux;
mod macos;

#[cfg(target_os = "linux")]
use linux::LinuxKeyStorage;
#[cfg(target_os = "macos")]
use macos::MacOSKeyStorage;

// for macos keychain service name
const SERVICE_NAME: &'static str = "hoot";

pub enum Error {
    IOError(std::io::Error),
    Addition(String),
    Removal(String),
    KeyNotFound,
    UnwrappingFailed(String),
}

pub type Result<T> = core::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::IOError(value)
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IOError(err) => write!(f, "IOError: {:?}", err),
            Error::Addition(key) => write!(f, "Could not add key {}", key),
            Error::Removal(key) => write!(f, "Could not remove key {}", key),
            Error::KeyNotFound => write!(f, "Could not find key in keystore"),
            Error::UnwrappingFailed(err) => {
                write!(f, "Couldn't unwrap gift wrapped event: {}", err)
            }
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IOError(err) => write!(f, "IO error: {}", err),
            Error::Addition(key) => write!(f, "Could not add key {}", key),
            Error::Removal(key) => write!(f, "Could not remove key {}", key),
            Error::KeyNotFound => write!(f, "Could not find key in keystore"),
            Error::UnwrappingFailed(err) => {
                write!(f, "Couldn't unwrap gift wrapped event: {}", err)
            }
        }
    }
}

pub enum KeyStorageType {
    None,
    #[cfg(target_os = "linux")]
    Linux,
    #[cfg(target_os = "macos")]
    MacOS,
}

pub trait KeyStorage {
    fn get_keys(&self) -> Result<Vec<Keys>>;
    fn add_key(&self, key: &Keys) -> Result<()>;
    fn remove_key(&self, key: &Keys) -> Result<()>;
}

impl KeyStorage for KeyStorageType {
    fn add_key(&self, key: &Keys) -> Result<()> {
        match self {
            Self::None => Ok(()),
            #[cfg(target_os = "linux")]
            Self::Linux => LinuxKeyStorage::new().add_key(key),
            #[cfg(target_os = "macos")]
            Self::MacOS => MacOSKeyStorage::new(SERVICE_NAME).add_key(key),
        }
    }

    fn get_keys(&self) -> Result<Vec<Keys>> {
        match self {
            Self::None => Ok(Vec::new()),
            #[cfg(target_os = "linux")]
            Self::Linux => LinuxKeyStorage::new().get_keys(),
            #[cfg(target_os = "macos")]
            Self::MacOS => MacOSKeyStorage::new(SERVICE_NAME).get_keys(),
        }
    }

    fn remove_key(&self, key: &Keys) -> Result<()> {
        match self {
            Self::None => Ok(()),
            #[cfg(target_os = "linux")]
            Self::Linux => LinuxKeyStorage::new().remove_key(key),
            #[cfg(target_os = "macos")]
            Self::MacOS => MacOSKeyStorage::new(SERVICE_NAME).remove_key(key),
        }
    }
}
