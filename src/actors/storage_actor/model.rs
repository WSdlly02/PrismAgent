use std::path::PathBuf;

pub(crate) mod agent;
pub(crate) mod context;
pub(crate) mod misc;
pub(crate) mod unit;
pub(crate) mod workflow;

pub struct StorageSubsystem {
    pub root: PathBuf,
}
