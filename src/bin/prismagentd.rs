use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Query, State},
    http::{Request, StatusCode, Uri},
    middleware,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::stream::{self, Stream, StreamExt};
use ipnetwork::IpNetwork;
use prismagent::{
    actors::{
        agent_actor::model::{AgentActor, AgentMsg},
        context_actor::model::{ContextActor, ContextHandle, ContextMsg},
        llm_actor::model::{LlmActor, LlmHandle, LlmMsg},
        profile_actor::model::{ProfileActor, ProfileHandle, ProfileMsg},
        shell_actor::model::{
            AgentAccessRequest, AuthorizedAgentCreateRequest, AuthorizedApproveRequest,
            AuthorizedSendMessageRequest, AuthorizedWorkflowCancelRequest, ShellActor, ShellHandle,
            ShellMsg, WorkspaceAccessRequest,
        },
        storage_actor::model::{StorageActor, StorageHandle, StorageMsg},
        tools_actor::model::{ToolsActor, ToolsHandle, ToolsMsg},
        workflow_actor::model::{WorkflowActor, WorkflowHandle, WorkflowMsg},
        workspace_actor::model::{
            AcquireLeaseRequest, ReleaseLeaseRequest, WorkspaceActor, WorkspaceCreateRequest,
            WorkspaceMsg,
        },
    },
    error::SubsystemError,
    handles::AppHandles,
    web_assets,
};
use serde_json::{Value, json};
use std::{convert::Infallible, net::SocketAddr, sync::OnceLock, time::Duration};
use tokio::sync::mpsc;

const DEFAULT_ADDR: &str = "0.0.0.0:7618";

const ALLOWED_NETS: &[&str] = &[
    "127.0.0.0/8",
    "::1/128",
    "192.168.0.0/16",
    "10.144.144.0/24",
];

fn allowed_nets() -> &'static [IpNetwork] {
    static ALLOWED_NETS_CACHE: OnceLock<Vec<IpNetwork>> = OnceLock::new();
    ALLOWED_NETS_CACHE
        .get_or_init(|| {
            ALLOWED_NETS
                .iter()
                .map(|s| s.parse().expect("invalid CIDR in ALLOWED_NETS"))
                .collect()
        })
        .as_slice()
}

async fn ip_filter(request: Request<Body>, next: middleware::Next) -> Result<Response, StatusCode> {
    let addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .ok_or(StatusCode::FORBIDDEN)?;

    if allowed_nets().iter().any(|net| net.contains(addr)) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

#[derive(Clone)]
struct AppState {
    shell: ShellHandle,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = std::env::var("PRISMAGENT_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let addr = addr.parse::<SocketAddr>()?;
    let state = AppState {
        shell: start_runtime()?,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/workspaces/list", get(list_workspaces))
        .route("/api/workspaces/add", post(add_workspace))
        .route("/api/workspaces/acquire_lease", post(acquire_lease))
        .route("/api/workspaces/release_lease", post(release_lease))
        .route("/api/profiles/list", get(list_profiles))
        .route("/api/workflows/cancel", post(workflow_cancel))
        .route("/api/agents/list", get(list_agents))
        .route("/api/agents/create", post(create_agent))
        .route("/api/agents/snapshot", get(agent_snapshot))
        .route("/api/agents/event_stream", get(agent_event_stream))
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

fn start_runtime() -> anyhow::Result<ShellHandle> {
    let (workspace_tx, workspace_rx) = mpsc::channel::<WorkspaceMsg>(64);
    let workspace =
        prismagent::actors::workspace_actor::model::WorkspaceHandle { tx: workspace_tx };
    let (storage_tx, storage_rx) = mpsc::channel::<StorageMsg>(64);
    let storage = StorageHandle { tx: storage_tx };
    let (context_tx, context_rx) = mpsc::channel::<ContextMsg>(64);
    let context = ContextHandle { tx: context_tx };
    let (profile_tx, profile_rx) = mpsc::channel::<ProfileMsg>(64);
    let profile = ProfileHandle { tx: profile_tx };
    let (llm_tx, llm_rx) = mpsc::channel::<LlmMsg>(64);
    let llm = LlmHandle { tx: llm_tx };
    let (tools_tx, tools_rx) = mpsc::channel::<ToolsMsg>(64);
    let tools = ToolsHandle { tx: tools_tx };
    let (workflow_tx, workflow_rx) = mpsc::channel::<WorkflowMsg>(64);
    let workflow = WorkflowHandle { tx: workflow_tx };
    let (agent_tx, agent_rx) = mpsc::channel::<AgentMsg>(64);
    let agent = prismagent::actors::agent_actor::model::AgentHandle { tx: agent_tx };
    let (shell_tx, shell_rx) = mpsc::channel::<ShellMsg>(64);
    let shell = ShellHandle { tx: shell_tx };
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

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn web_asset(uri: Uri) -> impl IntoResponse {
    web_assets::asset_response(uri.path())
}

async fn list_workspaces(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.list_workspaces().await?)))
}

async fn add_workspace(
    State(state): State<AppState>,
    Json(request): Json<WorkspaceCreateRequest>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.create_workspace(request).await?)))
}

async fn acquire_lease(
    State(state): State<AppState>,
    Json(request): Json<AcquireLeaseRequest>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.acquire_lease(request).await?)))
}

async fn release_lease(
    State(state): State<AppState>,
    Json(request): Json<ReleaseLeaseRequest>,
) -> ApiResult<Json<Value>> {
    state.shell.release_lease(request).await?;
    Ok(Json(json!({ "released": true })))
}

async fn list_profiles(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.list_profiles().await?)))
}

async fn workflow_cancel(
    State(state): State<AppState>,
    Json(request): Json<AuthorizedWorkflowCancelRequest>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.workflow_cancel(request).await?)))
}

async fn create_agent(
    State(state): State<AppState>,
    Json(request): Json<AuthorizedAgentCreateRequest>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.create_agent(request).await?)))
}

async fn list_agents(
    State(state): State<AppState>,
    Query(query): Query<WorkspaceAccessRequest>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.list_agents(query).await?)))
}

async fn agent_snapshot(
    State(state): State<AppState>,
    Query(query): Query<AgentAccessRequest>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.agent_snapshot(query).await?)))
}

async fn agent_event_stream(
    State(state): State<AppState>,
    Query(query): Query<AgentAccessRequest>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let receiver = state.shell.subscribe_agent(query).await?;
    let events = stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Some(event) => {
                let event = Event::default()
                    .event(event_name(&event))
                    .json_data(event)
                    .unwrap_or_else(|error| {
                        Event::default()
                            .event("error")
                            .data(format!("failed to encode event: {error}"))
                    });
                Some((Ok(event), receiver))
            }
            None => None,
        }
    });
    let connected = stream::once(async {
        Ok(Event::default()
            .event("connected")
            .data(r#"{"status":"connected"}"#))
    });
    Ok(Sse::new(connected.chain(events)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    ))
}

async fn send_message(
    State(state): State<AppState>,
    Json(request): Json<AuthorizedSendMessageRequest>,
) -> ApiResult<Json<Value>> {
    state.shell.send_message(request).await?;
    Ok(Json(json!({ "accepted": true })))
}

async fn approve_request(
    State(state): State<AppState>,
    Json(request): Json<AuthorizedApproveRequest>,
) -> ApiResult<Json<Value>> {
    state.shell.approve_request(request).await?;
    Ok(Json(json!({ "accepted": true })))
}

async fn cancel(
    State(state): State<AppState>,
    Json(query): Json<AgentAccessRequest>,
) -> ApiResult<Json<Value>> {
    state.shell.cancel(query).await?;
    Ok(Json(json!({ "cancelled": true })))
}

fn event_name(event: &prismagent::actors::agent_actor::model::AgentEvent) -> &'static str {
    use prismagent::actors::agent_actor::model::AgentEvent;
    match event {
        AgentEvent::UnitAppend { .. } => "unit_append",
        AgentEvent::StreamDelta { .. } => "stream_delta",
        AgentEvent::ApproveRequest { .. } => "approve_request",
        AgentEvent::StatusChanged { .. } => "status_changed",
        AgentEvent::Error { .. } => "error",
    }
}

type ApiResult<T> = Result<T, ApiError>;

struct ApiError(SubsystemError);

impl From<SubsystemError> for ApiError {
    fn from(error: SubsystemError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = match self.0 {
            SubsystemError::NotFound { .. } => StatusCode::NOT_FOUND,
            SubsystemError::Conflict { .. } => StatusCode::CONFLICT,
            SubsystemError::InvalidInput { .. } => StatusCode::BAD_REQUEST,
            SubsystemError::PermissionDenied { .. } => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}
