use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type SubsystemResult<T> = Result<T, SubsystemError>;

/// Stable, transport-facing error data exposed to API clients.
///
/// `SubsystemError` is an internal error used between actors. It may contain
/// implementation details and should not be serialized directly. REST and
/// WebSocket adapters both expose this smaller representation instead. This
/// conversion standardizes shape and classification; it does not redact the
/// diagnostic `message`, so internal errors must never contain secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Typed failure used by actor request/reply paths inside the process.
/// Transport boundaries must convert it to `PublicError` rather than serialize
/// it directly.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SubsystemError {
    #[error("actor has stopped: {actor}")]
    ActorDead { actor: &'static str },

    #[error("not found: {resource} {id}")]
    NotFound { resource: &'static str, id: String },

    #[error("conflict: {resource} {id}")]
    Conflict { resource: &'static str, id: String },

    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("permission denied: {action}")]
    PermissionDenied { action: &'static str },

    #[error("timeout: {operation}")]
    Timeout { operation: &'static str },

    #[error("io error: {message}")]
    Io { message: String },

    #[error("llm error: {message}")]
    Llm { message: String },

    #[error("tool error: {message}")]
    Tool { message: String },

    #[error("internal error: {message}")]
    Internal { message: String },
}

impl SubsystemError {
    pub fn actor_dead(actor: &'static str) -> Self {
        Self::ActorDead { actor }
    }

    pub fn not_found(resource: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            resource,
            id: id.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Converts an internal actor error into the stable error contract shared
    /// by REST and WebSocket transports.
    pub fn public_error(&self) -> PublicError {
        let code = match self {
            Self::ActorDead { .. } => "actor_unavailable".to_string(),
            Self::NotFound { resource, .. } => format!("{resource}_not_found"),
            Self::Conflict { resource, .. } => format!("{resource}_conflict"),
            Self::InvalidInput { .. } => "invalid_input".to_string(),
            Self::PermissionDenied { .. } => "permission_denied".to_string(),
            Self::Timeout { .. } => "timeout".to_string(),
            Self::Io { .. } => "io_error".to_string(),
            Self::Llm { .. } => "llm_error".to_string(),
            Self::Tool { .. } => "tool_error".to_string(),
            Self::Internal { .. } => "internal_error".to_string(),
        };
        let retryable = matches!(
            self,
            Self::ActorDead { .. }
                | Self::Conflict { .. }
                | Self::Timeout { .. }
                | Self::Io { .. }
                | Self::Llm { .. }
        );
        PublicError {
            code,
            message: self.to_string(),
            retryable,
        }
    }
}

impl From<std::io::Error> for SubsystemError {
    fn from(error: std::io::Error) -> Self {
        Self::io(error.to_string())
    }
}

impl From<toml::de::Error> for SubsystemError {
    fn from(error: toml::de::Error) -> Self {
        Self::invalid_input(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_error_keeps_resource_specific_code() {
        let error = SubsystemError::Conflict {
            resource: "workspace_lease",
            id: "workspace-1".to_string(),
        }
        .public_error();

        assert_eq!(error.code, "workspace_lease_conflict");
        assert_eq!(error.message, "conflict: workspace_lease workspace-1");
        assert!(error.retryable);
    }

    #[test]
    fn invalid_input_is_not_marked_retryable() {
        let error = SubsystemError::invalid_input("bad request").public_error();

        assert_eq!(error.code, "invalid_input");
        assert!(!error.retryable);
    }
}
