#[derive(Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum EventKind {
    ProfileMetadata = 0,
    MailEvent = 1059,
    Custom(u64),
}
