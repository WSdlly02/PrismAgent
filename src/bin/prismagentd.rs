use axum::{
    Json, Router,
    body::Body,
    extract::{
        ConnectInfo, FromRequest, FromRequestParts, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{Request, StatusCode, Uri, request::Parts},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use ipnetwork::IpNetwork;
use prismagent::{
    actors::{
        agent_actor::model::{AgentActor, AgentHandle, AgentMsg},
        context_actor::model::{ContextActor, ContextHandle, ContextMsg},
        llm_actor::model::{LlmActor, LlmHandle, LlmMsg},
        profile_actor::model::{ProfileActor, ProfileHandle, ProfileMsg},
        shell_actor::model::{
            AgentAccessRequest, AgentWriteAccessRequest, AuthorizedAgentCreateRequest,
            AuthorizedApproveRequest, AuthorizedCancelWorkflowRequest,
            AuthorizedDeleteWorkspaceRequest, AuthorizedSendMessageRequest, ConnectionId,
            ShellActor, ShellHandle, ShellMsg, WorkspaceAccessRequest, WsEvent,
        },
        storage_actor::model::{StorageActor, StorageHandle, StorageMsg},
        tools_actor::model::{ToolsActor, ToolsHandle, ToolsMsg},
        workflow_actor::model::{WorkflowActor, WorkflowHandle, WorkflowMsg},
        workspace_actor::model::{
            AcquireLeaseRequest, ReleaseLeaseRequest, WorkspaceActor, WorkspaceCreateRequest,
            WorkspaceHandle, WorkspaceMsg,
        },
    },
    error::{ErrorClass, PublicError, SubsystemError, SubsystemResult},
    handles::AppHandles,
    web_assets,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;

const DEFAULT_ADDR: &str = "0.0.0.0:7618";

const ALLOWED_NETS: &[&str] = &[
    "127.0.0.0/8",
    "::1/128",
    "192.168.0.0/16",
    "10.144.144.0/24",
];

fn allowed_nets() -> &'static [IpNetwork] {
    static ALLOWED_NETS_CACHE: std::sync::OnceLock<Vec<IpNetwork>> = std::sync::OnceLock::new();
    ALLOWED_NETS_CACHE
        .get_or_init(|| {
            ALLOWED_NETS
                .iter()
                .map(|s| s.parse().expect("invalid CIDR in ALLOWED_NETS"))
                .collect()
        })
        .as_slice()
}

async fn ip_filter(
    request: Request<Body>,
    next: middleware::Next,
) -> Result<Response, RestApiError> {
    let addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .ok_or(RestApiError(SubsystemError::PermissionDenied {
            action: "access PrismAgent without peer address information",
        }))?;

    if allowed_nets().iter().any(|net| net.contains(addr)) {
        Ok(next.run(request).await)
    } else {
        Err(RestApiError(SubsystemError::PermissionDenied {
            action: "access PrismAgent from outside the allowed networks",
        }))
    }
}

#[derive(Clone)]
struct AppState {
    shell: ShellHandle,
    next_connection_id: Arc<AtomicU64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = std::env::var("PRISMAGENT_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let addr = addr.parse::<SocketAddr>()?;
    let state = AppState {
        shell: start_runtime()?,
        next_connection_id: Arc::new(AtomicU64::new(1)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/api/workspaces/list", get(list_workspaces))
        .route("/api/workspaces/add", post(add_workspace))
        .route("/api/workspaces/acquire_lease", post(acquire_lease))
        .route("/api/workspaces/release_lease", post(release_lease))
        .route("/api/workspaces/delete", post(delete_workspace))
        .route("/api/profiles/list", get(list_profiles))
        .route("/api/workflows/cancel", post(workflow_cancel))
        .route("/api/agents/list", get(list_agents))
        .route("/api/agents/create", post(create_agent))
        .route("/api/agents/delete", post(delete_agent))
        .route("/api/agents/snapshot", get(agent_snapshot))
        .route("/api/agents/send_message", post(send_message))
        .route("/api/agents/approve_request", post(approve_request))
        .route("/api/agents/cancel", post(cancel))
        .fallback(get(web_asset))
        .layer(middleware::from_fn(ip_filter))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("PrismAgent daemon listening on http://{addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

// ============================================================================
// Daemon startup macros — reduce channel/handle/spawn boilerplate
// ============================================================================

/// Creates an mpsc channel, constructs the handle, and binds both the handle
/// variable and the receiver variable in the caller's scope.
///
/// The handle expression receives the sender half via `$tx_var` (hygiene-safe).
///
/// ```text
/// actor_channel!(workspace, workspace_rx, tx, WorkspaceMsg, WorkspaceHandle { tx });
/// ```
///
/// Expands to:
/// ```ignore
/// let (tx, rx) = mpsc::channel::<WorkspaceMsg>(64);
/// let workspace = WorkspaceHandle { tx };
/// let workspace_rx = rx;
/// ```
macro_rules! actor_channel {
    ($handle_var:ident, $rx_var:ident, $tx_var:ident, $Msg:ty, $handle:expr) => {
        let ($tx_var, rx) = mpsc::channel::<$Msg>(64);
        let $handle_var = $handle;
        let $rx_var = rx;
    };
}

/// Spawns an actor from its receiver.
///
/// Variants:
/// - `ok`     — `load(rx)` returns `SubsystemResult<Self>`, needs `?`
/// - `ok_handles` — `load(rx, handles)` returns `SubsystemResult<Self>`
/// - `go`     — `load(rx)` returns `Self` directly
/// - `go_handles` — `load(rx, handles)` returns `Self` directly
macro_rules! spawn_actor {
    (ok $Actor:ty, $rx:ident) => {
        <$Actor>::load($rx)?.spawn();
    };
    (ok_handles $Actor:ty, $rx:ident, $handles:expr) => {
        <$Actor>::load($rx, $handles)?.spawn();
    };
    (go $Actor:ty, $rx:ident) => {
        <$Actor>::load($rx).spawn();
    };
    (go_handles $Actor:ty, $rx:ident, $handles:expr) => {
        <$Actor>::load($rx, $handles).spawn();
    };
}

fn start_runtime() -> anyhow::Result<ShellHandle> {
    // Channel + handle creation
    actor_channel!(
        workspace,
        workspace_rx,
        tx,
        WorkspaceMsg,
        WorkspaceHandle { tx }
    );
    actor_channel!(storage, storage_rx, tx, StorageMsg, StorageHandle { tx });
    actor_channel!(context, context_rx, tx, ContextMsg, ContextHandle { tx });
    actor_channel!(profile, profile_rx, tx, ProfileMsg, ProfileHandle { tx });
    actor_channel!(llm, llm_rx, tx, LlmMsg, LlmHandle { tx });
    actor_channel!(tools, tools_rx, tx, ToolsMsg, ToolsHandle { tx });
    actor_channel!(
        workflow,
        workflow_rx,
        tx,
        WorkflowMsg,
        WorkflowHandle { tx }
    );
    actor_channel!(agent, agent_rx, tx, AgentMsg, AgentHandle { tx });
    actor_channel!(shell, shell_rx, tx, ShellMsg, ShellHandle { tx });

    let handles = AppHandles {
        profile,
        context,
        storage,
        workspace,
        agent,
        shell,
        llm,
        tools,
        workflow,
    };

    // Spawn actors: `ok` = load returns Result, `go` = load returns Self directly
    spawn_actor!(ok ProfileActor, profile_rx);
    spawn_actor!(ok StorageActor, storage_rx);
    spawn_actor!(ok_handles ContextActor, context_rx, handles.clone());
    spawn_actor!(ok WorkspaceActor, workspace_rx);
    spawn_actor!(go LlmActor, llm_rx);
    spawn_actor!(go_handles ToolsActor, tools_rx, handles.clone());
    spawn_actor!(go_handles WorkflowActor, workflow_rx, handles.clone());
    spawn_actor!(go_handles AgentActor, agent_rx, handles.clone());
    spawn_actor!(go_handles ShellActor, shell_rx, handles.clone());

    Ok(handles.shell)
}

// ========== WebSocket handler ==========

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    let connection_id = state.next_connection_id.fetch_add(1, Ordering::Relaxed);
    ws.on_upgrade(move |socket| handle_ws(socket, state.shell, connection_id))
}

async fn handle_ws(mut socket: WebSocket, shell: ShellHandle, connection_id: ConnectionId) {
    // Register connection with shell actor
    let mut event_rx = match shell.register_connection(connection_id).await {
        Ok(event_rx) => event_rx,
        Err(error) => {
            let event = WsEvent::Error {
                error: error.public_error(),
            };
            if let Ok(payload) = serde_json::to_string(&event) {
                let _ = socket.send(Message::Text(payload.into())).await;
            }
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Send connected confirmation
    let connected_msg = serde_json::to_string(&json!({ "type": "connected" })).unwrap();
    if ws_sender
        .send(Message::Text(connected_msg.into()))
        .await
        .is_err()
    {
        shell.unregister_connection(connection_id);
        return;
    }

    // Shared flag for coordinating shutdown
    let shutdown = Arc::new(tokio::sync::Notify::new());

    // Channel for heartbeat → write_task ping signaling
    let (ping_tx, mut ping_rx) = mpsc::channel::<()>(1);
    // Protocol errors originate in the read task after the socket is split.
    // This connection-local channel lets the write task return them without
    // routing transport concerns through ShellActor.
    let (connection_event_tx, mut connection_event_rx) = mpsc::channel::<WsEvent>(8);

    // Write task: forward events from shell actor → WS client, and send pings
    let _write_shell = shell.clone();
    let write_shutdown = shutdown.clone();
    let write_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    let json = serialize_ws_event(&event);
                    if ws_sender.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Some(event) = connection_event_rx.recv() => {
                    let json = serialize_ws_event(&event);
                    if ws_sender.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Some(()) = ping_rx.recv() => {
                    let ping_msg = serde_json::json!({"type": "ping", "ts": now_secs()});
                    if ws_sender.send(Message::Text(ping_msg.to_string().into())).await.is_err() {
                        break;
                    }
                }
                _ = write_shutdown.notified() => {
                    break;
                }
                else => break,
            }
        }
        let _ = ws_sender.close().await;
    });

    // Heartbeat task: check pong timeout every 15s, signal write_task to send ping
    let heartbeat_shell = shell.clone();
    let heartbeat_shutdown = shutdown.clone();
    let last_pong = Arc::new(AtomicU64::new(now_secs()));
    let heartbeat_last_pong = last_pong.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.tick().await; // skip first immediate tick
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if now_secs() - heartbeat_last_pong.load(Ordering::Relaxed) > 30 {
                        break;
                    }
                    if ping_tx.send(()).await.is_err() {
                        break;
                    }
                }
                _ = heartbeat_shutdown.notified() => {
                    break;
                }
            }
        }
        heartbeat_shell.unregister_connection(connection_id);
    });

    // Read task: read messages from WS client → shell actor
    let read_shell = shell.clone();
    let read_shutdown = shutdown.clone();
    let read_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Err(error) = handle_client_message(&read_shell, connection_id, &text, &last_pong).await {
                                let event = WsEvent::Error {
                                    error: error.public_error(),
                                };
                                // Unlike cross-actor event delivery, awaiting is safe here:
                                // this channel is local to one WebSocket connection and
                                // applies backpressure only to that connection's read task.
                                if connection_event_tx.send(event).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            last_pong.store(now_secs(), Ordering::Relaxed);
                        }
                        Some(Ok(Message::Ping(_data))) => {
                            // Ping is auto-responded by axum, but we track it
                            last_pong.store(now_secs(), Ordering::Relaxed);
                        }
                        Some(Err(e)) => {
                            eprintln!("WS read error: {e}");
                            break;
                        }
                        None => break,
                        _ => {} // Binary, Close, etc.
                    }
                }
                _ = read_shutdown.notified() => {
                    break;
                }
            }
        }
        read_shell.unregister_connection(connection_id);
    });

    // Wait for any task to finish, then signal shutdown
    tokio::select! {
        _ = write_task => {},
        _ = read_task => {},
        _ = heartbeat_task => {},
    }
    shutdown.notify_waiters();
}

async fn handle_client_message(
    shell: &ShellHandle,
    connection_id: ConnectionId,
    text: &str,
    last_pong: &AtomicU64,
) -> SubsystemResult<()> {
    let msg: serde_json::Value = serde_json::from_str(text).map_err(|error| {
        SubsystemError::validation_field("message", format!("invalid JSON: {error}"))
    })?;

    let msg_type = msg["type"].as_str().ok_or_else(|| {
        SubsystemError::validation_field("type", "missing or non-string message type")
    })?;

    match msg_type {
        "subscribe_workspace" => {
            let workspace_uuid = msg["workspace_uuid"]
                .as_str()
                .ok_or_else(|| {
                    SubsystemError::validation_field(
                        "workspace_uuid",
                        "missing or non-string workspace_uuid",
                    )
                })?
                .to_string();
            shell
                .subscribe_workspace(connection_id, workspace_uuid)
                .await?;
        }
        "unsubscribe_workspace" => {
            shell.unsubscribe_workspace(connection_id);
        }
        "subscribe_agent" => {
            let agent_uuid = msg["agent_uuid"]
                .as_str()
                .ok_or_else(|| {
                    SubsystemError::validation_field(
                        "agent_uuid",
                        "missing or non-string agent_uuid",
                    )
                })?
                .to_string();
            shell.subscribe_agent(connection_id, agent_uuid).await?;
        }
        "unsubscribe_agent" => {
            shell.unsubscribe_agent(connection_id);
        }
        "pong" => {
            last_pong.store(now_secs(), Ordering::Relaxed);
        }
        _ => {
            return Err(SubsystemError::validation_field(
                "type",
                format!("unknown message type: {msg_type}"),
            ));
        }
    }
    Ok(())
}

fn serialize_ws_event(event: &WsEvent) -> String {
    serde_json::to_string(event).unwrap_or_else(|error| {
        json!({
            "type": "error",
            "error": {
                "code": "serialization_error",
                "message": format!("serialize error: {error}"),
                "retryable": false,
            }
        })
        .to_string()
    })
}

use futures_util::{SinkExt, StreamExt};

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ========== REST endpoints ==========

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn web_asset(uri: Uri) -> impl IntoResponse {
    web_assets::asset_response(uri.path())
}

/// JSON request extractor that keeps Axum rejections inside the public REST
/// error contract instead of returning Axum's plain-text rejection body.
struct RestJson<T>(T);

impl<S, T> FromRequest<S> for RestJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = RestApiError;

    async fn from_request(request: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(|error| {
                RestApiError(SubsystemError::validation_field("body", error.to_string()))
            })?;
        Ok(Self(value))
    }
}

/// Query extractor counterpart to [`RestJson`].
struct RestQuery<T>(T);

impl<S, T> FromRequestParts<S> for RestQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = RestApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|error| {
                RestApiError(SubsystemError::validation_field("query", error.to_string()))
            })?;
        Ok(Self(value))
    }
}

async fn list_workspaces(State(state): State<AppState>) -> RestApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.list_workspaces().await?)))
}

async fn add_workspace(
    State(state): State<AppState>,
    RestJson(request): RestJson<WorkspaceCreateRequest>,
) -> RestApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.create_workspace(request).await?)))
}

async fn acquire_lease(
    State(state): State<AppState>,
    RestJson(request): RestJson<AcquireLeaseRequest>,
) -> RestApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.acquire_lease(request).await?)))
}

async fn release_lease(
    State(state): State<AppState>,
    RestJson(request): RestJson<ReleaseLeaseRequest>,
) -> RestApiResult<Json<Value>> {
    state.shell.release_lease(request).await?;
    Ok(Json(json!({ "released": true })))
}

async fn delete_workspace(
    State(state): State<AppState>,
    RestJson(request): RestJson<AuthorizedDeleteWorkspaceRequest>,
) -> RestApiResult<Json<Value>> {
    state.shell.delete_workspace(request).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn list_profiles(State(state): State<AppState>) -> RestApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.list_profiles().await?)))
}

async fn workflow_cancel(
    State(state): State<AppState>,
    RestJson(request): RestJson<AuthorizedCancelWorkflowRequest>,
) -> RestApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.workflow_cancel(request).await?)))
}

async fn create_agent(
    State(state): State<AppState>,
    RestJson(request): RestJson<AuthorizedAgentCreateRequest>,
) -> RestApiResult<Json<Value>> {
    state.shell.create_agent(request).await?;
    Ok(Json(json!({ "created": true })))
}

async fn delete_agent(
    State(state): State<AppState>,
    RestJson(request): RestJson<AgentWriteAccessRequest>,
) -> RestApiResult<Json<Value>> {
    state.shell.delete_agent(request).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn list_agents(
    State(state): State<AppState>,
    RestQuery(query): RestQuery<WorkspaceAccessRequest>,
) -> RestApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.list_agents(query).await?)))
}

async fn agent_snapshot(
    State(state): State<AppState>,
    RestQuery(query): RestQuery<AgentAccessRequest>,
) -> RestApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.agent_snapshot(query).await?)))
}

async fn send_message(
    State(state): State<AppState>,
    RestJson(request): RestJson<AuthorizedSendMessageRequest>,
) -> RestApiResult<Json<Value>> {
    state.shell.send_message(request).await?;
    Ok(Json(json!({ "accepted": true })))
}

async fn approve_request(
    State(state): State<AppState>,
    RestJson(request): RestJson<AuthorizedApproveRequest>,
) -> RestApiResult<Json<Value>> {
    state.shell.approve_request(request).await?;
    Ok(Json(json!({ "accepted": true })))
}

async fn cancel(
    State(state): State<AppState>,
    RestJson(query): RestJson<AgentWriteAccessRequest>,
) -> RestApiResult<Json<Value>> {
    state.shell.cancel(query).await?;
    Ok(Json(json!({ "cancelled": true })))
}

type RestApiResult<T> = Result<T, RestApiError>;

struct RestApiError(SubsystemError);

/// REST transport envelope. The caller already knows the requested operation
/// and resource IDs, so only the stable public error contract is repeated.
#[derive(Serialize)]
struct RestErrorResponse {
    error: PublicError,
}

impl From<SubsystemError> for RestApiError {
    fn from(error: SubsystemError) -> Self {
        Self(error)
    }
}

impl IntoResponse for RestApiError {
    fn into_response(self) -> axum::response::Response {
        let status = match self.0.descriptor().class {
            ErrorClass::BadRequest => StatusCode::BAD_REQUEST,
            ErrorClass::NotFound => StatusCode::NOT_FOUND,
            ErrorClass::Conflict => StatusCode::CONFLICT,
            ErrorClass::Forbidden => StatusCode::FORBIDDEN,
            ErrorClass::Unsupported => StatusCode::NOT_IMPLEMENTED,
            ErrorClass::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorClass::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorClass::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = RestErrorResponse {
            error: self.0.public_error(),
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use prismagent::error::{ConflictKind, ResourceKind};

    #[test]
    fn rest_api_error_uses_transport_neutral_error_class() {
        let cases = [
            (
                SubsystemError::validation("bad input"),
                StatusCode::BAD_REQUEST,
            ),
            (
                SubsystemError::not_found(ResourceKind::Agent, "agent-1"),
                StatusCode::NOT_FOUND,
            ),
            (
                SubsystemError::conflict(ConflictKind::AgentBusy, "agent-1"),
                StatusCode::CONFLICT,
            ),
            (
                SubsystemError::PermissionDenied {
                    action: "modify workspace",
                },
                StatusCode::FORBIDDEN,
            ),
            (
                SubsystemError::actor_dead("agent"),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                SubsystemError::Timeout {
                    operation: "inference",
                },
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                SubsystemError::Unsupported {
                    feature: "workflow cancellation",
                },
                StatusCode::NOT_IMPLEMENTED,
            ),
            (
                SubsystemError::configuration("profile", "missing API key"),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(RestApiError(error).into_response().status(), expected);
        }
    }

    #[tokio::test]
    async fn malformed_json_uses_the_public_rest_error_envelope() {
        let request = Request::builder()
            .header("content-type", "application/json")
            .body(Body::from("{"))
            .unwrap();
        let error = match RestJson::<WorkspaceCreateRequest>::from_request(request, &()).await {
            Ok(_) => panic!("malformed JSON should be rejected"),
            Err(error) => error,
        };

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["code"], "validation_failed");
        assert_eq!(body["error"]["retryable"], false);
    }

    #[tokio::test]
    async fn malformed_query_uses_the_public_rest_error_envelope() {
        let request = Request::builder()
            .uri("/api/agents/list")
            .body(Body::empty())
            .unwrap();
        let (mut parts, _) = request.into_parts();
        let error =
            match RestQuery::<WorkspaceAccessRequest>::from_request_parts(&mut parts, &()).await {
                Ok(_) => panic!("missing query fields should be rejected"),
                Err(error) => error,
            };

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["code"], "validation_failed");
    }

    #[tokio::test]
    async fn websocket_protocol_errors_keep_structured_error_data() {
        let (tx, _rx) = mpsc::channel(1);
        let shell = ShellHandle { tx };
        let last_pong = AtomicU64::new(0);

        let source = handle_client_message(&shell, 1, "not-json", &last_pong)
            .await
            .expect_err("invalid JSON should be rejected");
        let payload = serialize_ws_event(&WsEvent::Error {
            error: source.public_error(),
        });
        let payload: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(payload["type"], "error");
        assert_eq!(payload["error"]["code"], "validation_failed");
        assert_eq!(payload["error"]["retryable"], false);
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("invalid JSON")
        );
    }
}
