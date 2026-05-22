use std::path::PathBuf;

pub(crate) mod agent;
pub(crate) mod context;
pub(crate) mod unit;

pub struct StorageSubsystem {
    pub root: PathBuf,
}
