use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{
        Html, IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::stream::{self, Stream, StreamExt};
use prismagent::{
    actors::{
        agent_actor::model::{AgentActor, AgentMsg, ApproveRequest, SendMessageRequest},
        shell_actor::model::{ShellActor, ShellHandle, ShellMsg},
        workspace_actor::model::{
            AcquireLeaseRequest, AgentSummary, ReleaseLeaseRequest, WorkspaceActor, WorkspaceMsg,
        },
    },
    error::SubsystemError,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, time::Duration};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

const DEFAULT_ADDR: &str = "0.0.0.0:7618";

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
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/workspaces/list", get(list_workspaces))
        .route("/api/workspaces/acquire_lease", post(acquire_lease))
        .route("/api/workspaces/release_lease", post(release_lease))
        .route("/api/agents/snapshot", get(agent_snapshot))
        .route("/api/agents/event_stream", get(agent_event_stream))
        .route("/api/agents/send_message", post(send_message))
        .route("/api/agents/approve_request", post(approve_request))
        .route("/api/agents/cancel", post(cancel))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("PrismAgent daemon listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn start_runtime() -> anyhow::Result<ShellHandle> {
    let workspace_uuid = Uuid::now_v7().to_string();
    let agent_uuid = Uuid::now_v7().to_string();

    let (workspace_tx, workspace_rx) = mpsc::channel::<WorkspaceMsg>(64);
    let workspace =
        prismagent::actors::workspace_actor::model::WorkspaceHandle { tx: workspace_tx };
    WorkspaceActor::mock(
        workspace_rx,
        workspace_uuid,
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        vec![AgentSummary {
            agent_uuid: agent_uuid.clone(),
            agent_name: "agent-0".to_string(),
        }],
    )
    .spawn();

    let (agent_tx, agent_rx) = mpsc::channel::<AgentMsg>(64);
    let agent = prismagent::actors::agent_actor::model::AgentHandle { tx: agent_tx };
    AgentActor::mock(agent_rx, agent_uuid, "agent-0".to_string()).spawn();

    let (shell_tx, shell_rx) = mpsc::channel::<ShellMsg>(64);
    ShellActor::load(shell_rx, workspace, agent).spawn();
    Ok(ShellHandle { tx: shell_tx })
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn list_workspaces(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.shell.list_workspaces().await?)))
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

#[derive(Deserialize)]
struct AgentQuery {
    agent_uuid: String,
}

async fn agent_snapshot(
    State(state): State<AppState>,
    Query(query): Query<AgentQuery>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(
        state.shell.agent_snapshot(query.agent_uuid).await?
    )))
}

async fn agent_event_stream(
    State(state): State<AppState>,
    Query(query): Query<AgentQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let receiver = state.shell.subscribe_agent(query.agent_uuid).await?;
    let events = stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let event = Event::default()
                        .event(event_name(&event))
                        .json_data(event)
                        .unwrap_or_else(|error| {
                            Event::default()
                                .event("error")
                                .data(format!("failed to encode event: {error}"))
                        });
                    return Some((Ok(event), receiver));
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    return Some((
                        Ok(Event::default()
                            .event("error")
                            .data(format!("event stream lagged; skipped {skipped} events"))),
                        receiver,
                    ));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
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
    Json(request): Json<SendMessageRequest>,
) -> ApiResult<Json<Value>> {
    state.shell.send_message(request).await?;
    Ok(Json(json!({ "accepted": true })))
}

async fn approve_request(
    State(state): State<AppState>,
    Json(request): Json<ApproveRequest>,
) -> ApiResult<Json<Value>> {
    state.shell.approve_request(request).await?;
    Ok(Json(json!({ "accepted": true })))
}

async fn cancel(
    State(state): State<AppState>,
    Json(query): Json<AgentQuery>,
) -> ApiResult<Json<Value>> {
    state.shell.cancel(query.agent_uuid).await?;
    Ok(Json(json!({ "cancelled": true })))
}

fn event_name(event: &prismagent::actors::agent_actor::model::AgentEvent) -> &'static str {
    use prismagent::actors::agent_actor::model::AgentEvent;
    match event {
        AgentEvent::UnitAppend { .. } => "unit_append",
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

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>PrismAgent</title>
  <style>
    :root { font-family: system-ui, sans-serif; color: #182026; background: #f5f6f4; }
    * { box-sizing: border-box; }
    body { margin: 0; display: grid; grid-template-columns: 250px minmax(0, 1fr) 250px; min-height: 100vh; }
    aside { padding: 16px; border-right: 1px solid #d7dcda; background: #fff; }
    aside:last-child { border-right: 0; border-left: 1px solid #d7dcda; }
    main { display: grid; grid-template-rows: auto 1fr auto; min-width: 0; }
    header, form { padding: 14px 18px; border-bottom: 1px solid #d7dcda; background: #fff; }
    form { border-top: 1px solid #d7dcda; border-bottom: 0; display: flex; gap: 8px; }
    textarea { flex: 1; min-height: 48px; resize: vertical; padding: 10px; }
    button { padding: 8px 12px; cursor: pointer; }
    #messages { overflow: auto; padding: 18px; }
    .message { background: #fff; border: 1px solid #d7dcda; border-radius: 6px; padding: 10px; margin-bottom: 10px; white-space: pre-wrap; }
    .role { color: #0f766e; font-weight: 700; font-size: 12px; margin-bottom: 4px; }
    .agent { display: block; width: 100%; text-align: left; margin: 6px 0; }
    .muted { color: #66737a; font-size: 13px; }
    @media (max-width: 800px) { body { grid-template-columns: 180px minmax(0, 1fr); } aside:last-child { display: none; } }
  </style>
</head>
<body>
  <aside><h3>Workspaces</h3><div id="workspaces"></div></aside>
  <main>
    <header><strong>PrismAgent</strong> <span class="muted" id="status">loading</span></header>
    <section id="messages"></section>
    <form id="composer"><textarea id="input" placeholder="Send a message"></textarea><button>Send</button></form>
  </main>
  <aside><h3>Context</h3><p class="muted">Context and workflow views will appear here.</p></aside>
  <script>
    const workspaces = document.getElementById('workspaces');
    const messages = document.getElementById('messages');
    const status = document.getElementById('status');
    const input = document.getElementById('input');
    let agentUuid = null;
    let stream = null;

    async function api(path, options = {}) {
      const response = await fetch(path, { headers: { 'content-type': 'application/json' }, ...options });
      const body = await response.json();
      if (!response.ok) throw new Error(body.error || response.statusText);
      return body;
    }
    function append(unit) {
      const item = document.createElement('div');
      item.className = 'message';
      item.innerHTML = `<div class="role"></div><div class="content"></div>`;
      item.querySelector('.role').textContent = unit.role;
      item.querySelector('.content').textContent = unit.content;
      messages.appendChild(item);
      item.scrollIntoView({ block: 'end' });
    }
    async function openAgent(uuid) {
      agentUuid = uuid;
      const snapshot = await api(`/api/agents/snapshot?agent_uuid=${encodeURIComponent(uuid)}`);
      messages.textContent = '';
      snapshot.units.forEach(append);
      status.textContent = `${snapshot.agent_name} · ${snapshot.status}`;
      if (stream) stream.close();
      stream = new EventSource(`/api/agents/event_stream?agent_uuid=${encodeURIComponent(uuid)}`);
      stream.addEventListener('unit_append', event => append(JSON.parse(event.data).unit));
      stream.addEventListener('status_changed', event => status.textContent = JSON.parse(event.data).status);
      stream.addEventListener('approve_request', event => console.log('approve request', JSON.parse(event.data)));
    }
    async function loadWorkspaces() {
      const list = await api('/api/workspaces/list');
      workspaces.textContent = '';
      list.forEach(workspace => {
        const title = document.createElement('div');
        title.textContent = workspace.workspace_path;
        title.className = 'muted';
        workspaces.appendChild(title);
        workspace.agents.forEach(agent => {
          const button = document.createElement('button');
          button.className = 'agent';
          button.textContent = agent.agent_name;
          button.onclick = () => openAgent(agent.agent_uuid);
          workspaces.appendChild(button);
        });
      });
      const first = list[0]?.agents[0];
      if (first) openAgent(first.agent_uuid);
    }
    document.getElementById('composer').onsubmit = async event => {
      event.preventDefault();
      const text = input.value.trim();
      if (!text || !agentUuid) return;
      input.value = '';
      await api('/api/agents/send_message', {
        method: 'POST',
        body: JSON.stringify({ agent_uuid: agentUuid, message_body: { text, attachments: [] } }),
      });
    };
    loadWorkspaces().catch(error => status.textContent = error.message);
  </script>
</body>
</html>
"#;
