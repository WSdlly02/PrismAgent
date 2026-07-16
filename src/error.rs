use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

pub type SubsystemResult<T> = Result<T, SubsystemError>;

/// Stable, transport-facing error data exposed to authenticated API clients.
/// The complete diagnostic message is preserved; access control belongs at the
/// transport boundary rather than in the error model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Transport-neutral error class. REST maps this to an HTTP status while
/// WebSocket consumers use the same descriptor without depending on HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    BadRequest,
    NotFound,
    Conflict,
    Forbidden,
    Unsupported,
    Unavailable,
    Timeout,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorDescriptor {
    pub code: &'static str,
    pub class: ErrorClass,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Workspace,
    WorkspaceLease,
    Agent,
    Profile,
    Skill,
    Context,
    Workflow,
    WorkflowRuntime,
    File,
}

impl ResourceKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::WorkspaceLease => "workspace lease",
            Self::Agent => "agent",
            Self::Profile => "profile",
            Self::Skill => "skill",
            Self::Context => "context",
            Self::Workflow => "workflow",
            Self::WorkflowRuntime => "workflow runtime",
            Self::File => "file",
        }
    }

    const fn not_found_code(self) -> &'static str {
        match self {
            Self::Workspace => "workspace_not_found",
            Self::WorkspaceLease => "workspace_lease_not_found",
            Self::Agent => "agent_not_found",
            Self::Profile => "profile_not_found",
            Self::Skill => "skill_not_found",
            Self::Context => "context_not_found",
            Self::Workflow => "workflow_not_found",
            Self::WorkflowRuntime => "workflow_runtime_not_found",
            Self::File => "file_not_found",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    WorkspacePathExists,
    WorkspaceLeaseHeld,
    AgentBusy,
    FileAlreadyExists,
    ConcurrentModification,
}

impl ConflictKind {
    const fn label(self) -> &'static str {
        match self {
            Self::WorkspacePathExists => "workspace path already exists",
            Self::WorkspaceLeaseHeld => "workspace lease is held",
            Self::AgentBusy => "agent is busy",
            Self::FileAlreadyExists => "file already exists",
            Self::ConcurrentModification => "concurrent modification",
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::WorkspacePathExists => "workspace_path_exists",
            Self::WorkspaceLeaseHeld => "workspace_lease_conflict",
            Self::AgentBusy => "agent_busy",
            Self::FileAlreadyExists => "file_already_exists",
            Self::ConcurrentModification => "concurrent_modification",
        }
    }

    const fn retryable(self) -> bool {
        matches!(
            self,
            Self::WorkspaceLeaseHeld | Self::AgentBusy | Self::ConcurrentModification
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalKind {
    Llm,
}

impl ExternalKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Llm => "llm",
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::Llm => "llm_error",
        }
    }
}

/// Typed failure used by actor request/reply paths inside the process.
/// Transport boundaries convert it to [`PublicError`] through one descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SubsystemError {
    #[error("actor has stopped: {actor}")]
    ActorDead { actor: &'static str },

    #[error("{} not found: {id}", resource.label())]
    NotFound { resource: ResourceKind, id: String },

    #[error("{}: {id}", kind.label())]
    Conflict { kind: ConflictKind, id: String },

    #[error("validation failed{field_suffix}: {message}", field_suffix = field.map(|value| format!(" for {value}")).unwrap_or_default())]
    Validation {
        field: Option<&'static str>,
        message: String,
    },

    #[error("permission denied: {action}")]
    PermissionDenied { action: &'static str },

    #[error("timeout: {operation}")]
    Timeout { operation: &'static str },

    #[error("configuration error in {component}: {message}")]
    Configuration {
        component: &'static str,
        message: String,
    },

    #[error("corrupt state in {resource}: {message}")]
    CorruptState {
        resource: &'static str,
        message: String,
    },

    #[error("io error during {operation}{path_suffix}: {message}", path_suffix = path.as_ref().map(|value| format!(" at {}", value.display())).unwrap_or_default())]
    Io {
        operation: &'static str,
        path: Option<PathBuf>,
        kind: std::io::ErrorKind,
        message: String,
    },

    #[error("{} error: {message}", kind.label())]
    External {
        kind: ExternalKind,
        class: ErrorClass,
        message: String,
        retryable: bool,
    },

    #[error("unsupported operation: {feature}")]
    Unsupported { feature: &'static str },

    #[error("internal error during {operation}: {message}")]
    Internal {
        operation: &'static str,
        message: String,
    },
}

impl SubsystemError {
    pub fn actor_dead(actor: &'static str) -> Self {
        Self::ActorDead { actor }
    }

    pub fn not_found(resource: ResourceKind, id: impl Into<String>) -> Self {
        Self::NotFound {
            resource,
            id: id.into(),
        }
    }

    pub fn conflict(kind: ConflictKind, id: impl Into<String>) -> Self {
        Self::Conflict {
            kind,
            id: id.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            field: None,
            message: message.into(),
        }
    }

    pub fn validation_field(field: &'static str, message: impl Into<String>) -> Self {
        Self::Validation {
            field: Some(field),
            message: message.into(),
        }
    }

    pub fn configuration(component: &'static str, message: impl Into<String>) -> Self {
        Self::Configuration {
            component,
            message: message.into(),
        }
    }

    pub fn corrupt_state(resource: &'static str, message: impl Into<String>) -> Self {
        Self::CorruptState {
            resource,
            message: message.into(),
        }
    }

    pub fn io(operation: &'static str, path: Option<PathBuf>, error: std::io::Error) -> Self {
        Self::Io {
            operation,
            path,
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    pub fn external(
        kind: ExternalKind,
        class: ErrorClass,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self::External {
            kind,
            class,
            message: message.into(),
            retryable,
        }
    }

    pub fn internal(operation: &'static str, message: impl Into<String>) -> Self {
        Self::Internal {
            operation,
            message: message.into(),
        }
    }

    pub fn descriptor(&self) -> ErrorDescriptor {
        match self {
            Self::ActorDead { .. } => ErrorDescriptor {
                code: "actor_unavailable",
                class: ErrorClass::Unavailable,
                retryable: true,
            },
            Self::NotFound { resource, .. } => ErrorDescriptor {
                code: resource.not_found_code(),
                class: ErrorClass::NotFound,
                retryable: false,
            },
            Self::Conflict { kind, .. } => ErrorDescriptor {
                code: kind.code(),
                class: ErrorClass::Conflict,
                retryable: kind.retryable(),
            },
            Self::Validation { .. } => ErrorDescriptor {
                code: "validation_failed",
                class: ErrorClass::BadRequest,
                retryable: false,
            },
            Self::PermissionDenied { .. } => ErrorDescriptor {
                code: "permission_denied",
                class: ErrorClass::Forbidden,
                retryable: false,
            },
            Self::Timeout { .. } => ErrorDescriptor {
                code: "timeout",
                class: ErrorClass::Timeout,
                retryable: true,
            },
            Self::Configuration { .. } => ErrorDescriptor {
                code: "configuration_error",
                class: ErrorClass::Internal,
                retryable: false,
            },
            Self::CorruptState { .. } => ErrorDescriptor {
                code: "corrupt_state",
                class: ErrorClass::Internal,
                retryable: false,
            },
            Self::Io { kind, .. } => ErrorDescriptor {
                code: "io_error",
                class: ErrorClass::Internal,
                retryable: io_error_is_retryable(*kind),
            },
            Self::External {
                kind,
                class,
                retryable,
                ..
            } => ErrorDescriptor {
                code: kind.code(),
                class: *class,
                retryable: *retryable,
            },
            Self::Unsupported { .. } => ErrorDescriptor {
                code: "unsupported_operation",
                class: ErrorClass::Unsupported,
                retryable: false,
            },
            Self::Internal { .. } => ErrorDescriptor {
                code: "internal_error",
                class: ErrorClass::Internal,
                retryable: false,
            },
        }
    }

    pub fn public_error(&self) -> PublicError {
        let descriptor = self.descriptor();
        PublicError {
            code: descriptor.code.to_string(),
            message: self.to_string(),
            retryable: descriptor.retryable,
        }
    }
}

fn io_error_is_retryable(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_lease_conflict_has_stable_transport_semantics() {
        let error = SubsystemError::conflict(ConflictKind::WorkspaceLeaseHeld, "workspace-1");

        assert_eq!(
            error.descriptor(),
            ErrorDescriptor {
                code: "workspace_lease_conflict",
                class: ErrorClass::Conflict,
                retryable: true,
            }
        );
    }

    #[test]
    fn validation_is_a_non_retryable_bad_request() {
        let error = SubsystemError::validation("bad request");

        assert_eq!(error.descriptor().class, ErrorClass::BadRequest);
        assert_eq!(error.public_error().code, "validation_failed");
        assert!(!error.public_error().retryable);
    }

    #[test]
    fn configuration_and_corrupt_state_are_not_client_errors() {
        for error in [
            SubsystemError::configuration("profile", "missing API key"),
            SubsystemError::corrupt_state("agent file", "invalid JSON"),
        ] {
            assert_eq!(error.descriptor().class, ErrorClass::Internal);
            assert!(!error.descriptor().retryable);
        }
    }

    #[test]
    fn public_error_preserves_full_io_diagnostic() {
        let error = SubsystemError::io(
            "read profile",
            Some(PathBuf::from("/secret/server/path/profile.toml")),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );

        assert!(!error.descriptor().retryable);
        assert!(error.public_error().message.contains("read profile"));
        assert!(
            error
                .public_error()
                .message
                .contains("/secret/server/path/profile.toml")
        );
    }

    #[test]
    fn external_error_uses_explicit_classification() {
        let error = SubsystemError::external(
            ExternalKind::Llm,
            ErrorClass::Internal,
            "provider rejected credentials",
            false,
        );

        assert_eq!(error.descriptor().class, ErrorClass::Internal);
        assert!(!error.public_error().retryable);
        assert!(
            error
                .public_error()
                .message
                .contains("provider rejected credentials")
        );
    }
}
