use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, Uri},
    routing::post,
};
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command, sync::mpsc};

#[tokio::test]
async fn sends_stdin_verbatim_and_releases_lease_even_if_rejected() {
    let (tx, mut rx) = mpsc::channel(8);
    let app =
        Router::new()
            .fallback(post(
                |State(tx): State<mpsc::Sender<(String, Value)>>,
                 uri: Uri,
                 Json(body): Json<Value>| async move {
                    let (status, response) = match uri.path() {
                        "/api/workspaces/acquire_lease" => {
                            (StatusCode::OK, json!({"lease_token":"token"}))
                        }
                        "/api/agents/send_message" if body["agent_uuid"] == "busy" => {
                            (StatusCode::CONFLICT, json!({"error":{"code":"agent_busy"}}))
                        }
                        "/api/agents/send_message" => (StatusCode::OK, json!({"accepted":true})),
                        "/api/workspaces/release_lease" => {
                            (StatusCode::OK, json!({"released":true}))
                        }
                        _ => (StatusCode::NOT_FOUND, Value::Null),
                    };
                    tx.send((uri.path().to_owned(), body)).await.unwrap();
                    (status, Json(response))
                },
            ))
            .with_state(tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for agent in ["idle", "busy"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_prismagent-sender"))
            .args(["--server", &server, &format!("ws/{agent}"), "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let message = "  line one\nline two\n";
        child
            .stdin
            .take()
            .unwrap()
            .write_all(message.as_bytes())
            .await
            .unwrap();
        let result = child.wait_with_output().await.unwrap();
        assert_eq!(result.status.success(), agent == "idle");
        if agent == "busy" {
            assert!(
                String::from_utf8(result.stderr)
                    .unwrap()
                    .contains("agent_busy")
            );
        }
        let (path, body) = rx.try_recv().unwrap();
        assert_eq!(path, "/api/workspaces/acquire_lease");
        assert_eq!(body["workspace_uuid"], "ws");
        assert_eq!(
            rx.try_recv().unwrap(),
            (
                "/api/agents/send_message".into(),
                json!({
                    "workspace_uuid":"ws", "agent_uuid":agent, "lease_token":"token",
                    "message_body":{"text":message, "attachments":[]}
                })
            )
        );
        assert_eq!(
            rx.try_recv().unwrap(),
            (
                "/api/workspaces/release_lease".into(),
                json!({"workspace_uuid":"ws","lease_token":"token"})
            )
        );
    }
    task.abort();
}
