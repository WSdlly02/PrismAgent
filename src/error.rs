use thiserror::Error;

pub type SubsystemResult<T> = Result<T, SubsystemError>;

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
