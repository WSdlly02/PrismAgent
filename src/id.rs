use crate::error::{SubsystemError, SubsystemResult};
use std::collections::HashSet;

pub const DEFAULT_PETNAME_WORDS: u8 = 3;

pub fn petname_uuid(existing: impl IntoIterator<Item = String>) -> SubsystemResult<String> {
    let existing = existing.into_iter().collect::<HashSet<_>>();
    for _ in 0..32 {
        let Some(candidate) = petname::petname(DEFAULT_PETNAME_WORDS, "-") else {
            return Err(SubsystemError::internal("failed to generate petname uuid"));
        };
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(SubsystemError::internal(
        "failed to generate unique petname uuid",
    ))
}
