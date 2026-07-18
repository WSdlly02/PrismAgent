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
use futures_util::{SinkExt, StreamExt};
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
    future::IntoFuture,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

const DEFAULT_ADDR: &str = "0.0.0.0:7618";

const ALLOWED_NETS: &[&str] = &[
    "127.0.0.0/8",
    "::1/128",
    "192.168.0.0/16",
    "10.144.144.0/24",
];

fn allowed_nets() -> &'static [IpNetwork] {
    static CACHE: std::sync::LazyLock<Vec<IpNetwork>> = std::sync::LazyLock::new(|| {
        ALLOWED_NETS
            .iter()
            .map(|s| s.parse().expect("invalid CIDR in ALLOWED_NETS"))
            .collect()
    });
    CACHE.as_slice()
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
    let shell = start_runtime()?;
    let state = AppState {
        shell: shell.clone(),
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
    let (graceful_shutdown_tx, graceful_shutdown_rx) = oneshot::channel();
    let mut server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = graceful_shutdown_rx.await;
    })
    .into_future();
    // `tokio::pin!(server)` is not necessary because `into_future` already returns a pinned future

    let mut signals = shutdown_signals();
    let first_signal = tokio::select! {
        result = &mut server => {
            result?;
            return Ok(());
        }
        signal = signals.recv() => signal,
    };
    if first_signal.is_none() {
        eprintln!("shutdown signal listener stopped unexpectedly; shutting down");
        let _ = graceful_shutdown_tx.send(());
        server.await?;
        return Ok(());
    }

    eprintln!(
        "shutdown requested; rejecting new work and waiting for active agents \
         (send another shutdown signal to force exit)"
    );
    let decision = tokio::select! {
        result = &mut server => {
            result?;
            return Ok(());
        }
        decision = wait_for_agents_to_idle(&shell, &mut signals) => decision,
    };
    if decision == ShutdownDecision::Force {
        force_shutdown();
    }

    let _ = graceful_shutdown_tx.send(());
    tokio::select! {
        result = &mut server => result?,
        _ = wait_for_shutdown_signal(&mut signals) => force_shutdown(),
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShutdownDecision {
    Drain,
    Force,
}

async fn wait_for_agents_to_idle(
    shell: &ShellHandle,
    signals: &mut mpsc::UnboundedReceiver<()>,
) -> ShutdownDecision {
    loop {
        tokio::select! {
            _ = wait_for_shutdown_signal(signals) => return ShutdownDecision::Force,
            result = shell.try_shutdown() => {
                match result {
                    Ok(true) => {
                        eprintln!("all agents are idle; shutting down");
                        return ShutdownDecision::Drain;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!(
                            "failed to check whether agents are idle; \
                             proceeding with HTTP shutdown: {error}"
                        );
                        return ShutdownDecision::Drain;
                    }
                }
            }
        }

        tokio::select! {
            _ = wait_for_shutdown_signal(signals) => return ShutdownDecision::Force,
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
}

async fn wait_for_shutdown_signal(signals: &mut mpsc::UnboundedReceiver<()>) {
    match signals.recv().await {
        Some(()) => {}
        None => std::future::pending::<()>().await,
    }
}

fn force_shutdown() -> ! {
    eprintln!("second shutdown signal received; forcing exit");
    std::process::exit(130);
}

fn shutdown_signals() -> mpsc::UnboundedReceiver<()> {
    let (tx, rx) = mpsc::unbounded_channel();

    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let mut interrupt = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        let mut quit = signal(SignalKind::quit()).expect("install SIGQUIT handler");

        loop {
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = term.recv() => {}
                _ = quit.recv() => {}
            }
            if tx.send(()).is_err() {
                break;
            }
        }
    });

    #[cfg(not(unix))]
    tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() || tx.send(()).is_err() {
                break;
            }
        }
    });

    rx
}

// ============================================================================
// Daemon startup — create channels, build handles, spawn actors
// ============================================================================

fn start_runtime() -> anyhow::Result<ShellHandle> {
    let (tx, workspace_rx) = mpsc::channel::<WorkspaceMsg>(64);
    let workspace = WorkspaceHandle { tx };

    let (tx, storage_rx) = mpsc::channel::<StorageMsg>(64);
    let storage = StorageHandle { tx };

    let (tx, context_rx) = mpsc::channel::<ContextMsg>(64);
    let context = ContextHandle { tx };

    let (tx, profile_rx) = mpsc::channel::<ProfileMsg>(64);
    let profile = ProfileHandle { tx };

    let (tx, llm_rx) = mpsc::channel::<LlmMsg>(64);
    let llm = LlmHandle { tx };

    let (tx, tools_rx) = mpsc::channel::<ToolsMsg>(64);
    let tools = ToolsHandle { tx };

    let (tx, workflow_rx) = mpsc::channel::<WorkflowMsg>(64);
    let workflow = WorkflowHandle { tx };

    let (tx, agent_rx) = mpsc::channel::<AgentMsg>(64);
    let agent = AgentHandle { tx };

    let (tx, shell_rx) = mpsc::channel::<ShellMsg>(64);
    let shell = ShellHandle { tx };

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

    ProfileActor::load(profile_rx)?.spawn();
    StorageActor::load(storage_rx)?.spawn();
    ContextActor::load(context_rx, handles.clone())?.spawn();
    WorkspaceActor::load(workspace_rx)?.spawn();
    LlmActor::load(llm_rx).spawn();
    ToolsActor::load(tools_rx, handles.clone()).spawn();
    WorkflowActor::load(workflow_rx, handles.clone()).spawn();
    AgentActor::load(agent_rx, handles.clone()).spawn();
    ShellActor::load(shell_rx, handles.clone()).spawn();

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

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
    async fn shutdown_actor_failure_proceeds_to_http_drain() {
        let (shell_tx, shell_rx) = mpsc::channel(1);
        drop(shell_rx);
        let shell = ShellHandle { tx: shell_tx };
        let (_signal_tx, mut signals) = mpsc::unbounded_channel();

        let decision = tokio::time::timeout(
            Duration::from_secs(1),
            wait_for_agents_to_idle(&shell, &mut signals),
        )
        .await
        .expect("dead shell actor should not stall shutdown");

        assert_eq!(decision, ShutdownDecision::Drain);
    }

    #[tokio::test]
    async fn second_signal_forces_exit_while_waiting_for_agents() {
        let (shell_tx, _shell_rx) = mpsc::channel(1);
        let shell = ShellHandle { tx: shell_tx };
        let (signal_tx, mut signals) = mpsc::unbounded_channel();
        signal_tx.send(()).unwrap();

        let decision = tokio::time::timeout(
            Duration::from_secs(1),
            wait_for_agents_to_idle(&shell, &mut signals),
        )
        .await
        .expect("second signal should interrupt agent readiness wait");

        assert_eq!(decision, ShutdownDecision::Force);
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
