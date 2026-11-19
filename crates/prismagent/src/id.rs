use crate::error::{SubsystemError, SubsystemResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Agent IDs are scoped to a workspace, including in process-local routing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentAddr {
    pub workspace_uuid: String,
    pub agent_uuid: String,
}

impl AgentAddr {
    pub fn new(workspace_uuid: impl Into<String>, agent_uuid: impl Into<String>) -> Self {
        Self {
            workspace_uuid: workspace_uuid.into(),
            agent_uuid: agent_uuid.into(),
        }
    }
}

impl std::fmt::Display for AgentAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.workspace_uuid, self.agent_uuid)
    }
}

pub const DEFAULT_PETNAME_WORDS: u8 = 3;

pub fn petname_uuid(existing: impl IntoIterator<Item = String>) -> SubsystemResult<String> {
    let existing = existing.into_iter().collect::<HashSet<_>>();
    for _ in 0..32 {
        let Some(candidate) = petname::petname(DEFAULT_PETNAME_WORDS, "-") else {
            return Err(SubsystemError::internal(
                "generate petname uuid",
                "petname generator returned no candidate",
            ));
        };
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(SubsystemError::internal(
        "generate petname uuid",
        "failed to generate a unique candidate after 32 attempts",
    ))
}
