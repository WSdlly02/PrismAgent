use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, oneshot};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubsystemName {
    Agent,
    Config,
    Context,
    Llm,
    Storage,
    Shell,
    Tools,
    Workflow,
}

impl Display for SubsystemName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Agent => write!(f, "agent"),
            Self::Config => write!(f, "config"),
            Self::Context => write!(f, "context"),
            Self::Llm => write!(f, "llm"),
            Self::Storage => write!(f, "storage"),
            Self::Shell => write!(f, "shell"),
            Self::Tools => write!(f, "tools"),
            Self::Workflow => write!(f, "workflow"),
        }
    }
}

#[derive(Debug)]
pub struct Request {
    pub id: String,
    pub method: Method,
    pub from: SubsystemName,
    pub to: SubsystemName,
    pub path: String,
    pub body: Value,
    pub reply: ReplyChannel,
}

impl Request {
    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.body.clone())
    }

    pub fn respond(self, response: Response) {
        if let ReplyChannel::Once(tx) = self.reply {
            let _ = tx.send(response);
        }
    }
}

#[derive(Debug)]
pub enum ReplyChannel {
    Once(oneshot::Sender<Response>),
    Stream(mpsc::Sender<StreamChunk>),
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamChunk {
    Delta(Value),
    Done,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseStatus {
    Ok,
    BadRequest,
    NotFound,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub status: ResponseStatus,
    pub body: Value,
}

impl Response {
    pub fn ok(body: Value) -> Self {
        Self {
            status: ResponseStatus::Ok,
            body,
        }
    }

    pub fn bad_request(error: impl ToString) -> Self {
        Self {
            status: ResponseStatus::BadRequest,
            body: json!({ "error": error.to_string() }),
        }
    }

    pub fn not_found(path: impl ToString) -> Self {
        Self {
            status: ResponseStatus::NotFound,
            body: json!({ "error": format!("route not found: {}", path.to_string()) }),
        }
    }

    pub fn internal_error(error: impl ToString) -> Self {
        Self {
            status: ResponseStatus::InternalError,
            body: json!({ "error": error.to_string() }),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status == ResponseStatus::Ok
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusError {
    SubsystemNotFound(SubsystemName),
    RequestChannelClosed(SubsystemName),
    ResponseChannelClosed(SubsystemName),
}

impl Display for BusError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SubsystemNotFound(name) => write!(f, "subsystem not found: {name}"),
            Self::RequestChannelClosed(name) => {
                write!(f, "request channel closed for subsystem: {name}")
            }
            Self::ResponseChannelClosed(name) => {
                write!(f, "response channel closed for subsystem: {name}")
            }
        }
    }
}

impl Error for BusError {}

pub type BusResult<T> = Result<T, BusError>;

#[derive(Debug, Clone, Default)]
pub struct Bus {
    routes: Arc<RwLock<HashMap<SubsystemName, mpsc::Sender<Request>>>>,
}

impl Bus {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, name: SubsystemName, tx: mpsc::Sender<Request>) {
        self.routes.write().await.insert(name, tx);
    }

    pub async fn get(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        path: impl Into<String>,
    ) -> BusResult<Response> {
        self.send_once(to, from, Method::Get, path, Value::Null)
            .await
    }

    pub async fn post(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        path: impl Into<String>,
        body: impl Into<Value>,
    ) -> BusResult<Response> {
        self.send_once(to, from, Method::Post, path, body.into())
            .await
    }

    pub async fn put(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        path: impl Into<String>,
        body: impl Into<Value>,
    ) -> BusResult<Response> {
        self.send_once(to, from, Method::Put, path, body.into())
            .await
    }

    pub async fn patch(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        path: impl Into<String>,
        body: impl Into<Value>,
    ) -> BusResult<Response> {
        self.send_once(to, from, Method::Patch, path, body.into())
            .await
    }

    pub async fn delete(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        path: impl Into<String>,
        body: impl Into<Value>,
    ) -> BusResult<Response> {
        self.send_once(to, from, Method::Delete, path, body.into())
            .await
    }

    pub async fn post_stream(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        path: impl Into<String>,
        body: impl Into<Value>,
    ) -> BusResult<mpsc::Receiver<StreamChunk>> {
        let (tx, rx) = mpsc::channel(64);
        let request = Request {
            id: Uuid::now_v7().to_string(),
            method: Method::Post,
            from,
            to,
            path: path.into(),
            body: body.into(),
            reply: ReplyChannel::Stream(tx),
        };
        self.send(to, request).await?;
        Ok(rx)
    }

    pub async fn notify(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        path: impl Into<String>,
        body: impl Into<Value>,
    ) -> BusResult<()> {
        let request = Request {
            id: Uuid::now_v7().to_string(),
            method: Method::Post,
            from,
            to,
            path: path.into(),
            body: body.into(),
            reply: ReplyChannel::None,
        };
        self.send(to, request).await
    }

    async fn send_once(
        &self,
        to: SubsystemName,
        from: SubsystemName,
        method: Method,
        path: impl Into<String>,
        body: Value,
    ) -> BusResult<Response> {
        let (tx, rx) = oneshot::channel();
        let request = Request {
            id: Uuid::now_v7().to_string(),
            method,
            from,
            to,
            path: path.into(),
            body,
            reply: ReplyChannel::Once(tx),
        };
        self.send(to, request).await?;
        rx.await.map_err(|_| BusError::ResponseChannelClosed(to))
    }

    async fn send(&self, to: SubsystemName, request: Request) -> BusResult<()> {
        let tx = {
            let routes = self.routes.read().await;
            routes.get(&to).cloned()
        }
        .ok_or(BusError::SubsystemNotFound(to))?;

        tx.send(request)
            .await
            .map_err(|_| BusError::RequestChannelClosed(to))
    }
}

pub trait Subsystem: Send + 'static {
    fn name(&self) -> SubsystemName;
    fn start(self, bus: Bus) -> mpsc::Sender<Request>
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn routes_once_request_to_registered_subsystem() {
        let bus = Bus::new();
        let (tx, mut rx) = mpsc::channel(8);
        bus.register(SubsystemName::Llm, tx).await;

        tokio::spawn(async move {
            let req = rx.recv().await.unwrap();
            assert_eq!(req.method, Method::Get);
            assert_eq!(req.from, SubsystemName::Shell);
            assert_eq!(req.to, SubsystemName::Llm);
            assert_eq!(req.path, "ping");
            req.respond(Response::ok(json!({ "pong": true })));
        });

        let res = bus
            .get(SubsystemName::Llm, SubsystemName::Shell, "ping")
            .await
            .unwrap();
        assert_eq!(res.status, ResponseStatus::Ok);
        assert_eq!(res.body, json!({ "pong": true }));
    }

    #[tokio::test]
    async fn routes_stream_request_to_registered_subsystem() {
        let bus = Bus::new();
        let (tx, mut rx) = mpsc::channel(8);
        bus.register(SubsystemName::Tools, tx).await;

        tokio::spawn(async move {
            let req = rx.recv().await.unwrap();
            assert_eq!(req.method, Method::Post);
            match req.reply {
                ReplyChannel::Stream(tx) => {
                    tx.send(StreamChunk::Delta(json!({ "token": "a" })))
                        .await
                        .unwrap();
                    tx.send(StreamChunk::Done).await.unwrap();
                }
                _ => panic!("expected stream reply channel"),
            }
        });

        let mut stream = bus
            .post_stream(
                SubsystemName::Tools,
                SubsystemName::Shell,
                "tokens",
                json!({ "prompt": "hello" }),
            )
            .await
            .unwrap();

        assert_eq!(
            stream.recv().await,
            Some(StreamChunk::Delta(json!({ "token": "a" })))
        );
        assert_eq!(stream.recv().await, Some(StreamChunk::Done));
    }

    #[tokio::test]
    async fn sends_notify_without_reply_channel() {
        let bus = Bus::new();
        let (tx, mut rx) = mpsc::channel(8);
        bus.register(SubsystemName::Agent, tx).await;

        bus.notify(
            SubsystemName::Agent,
            SubsystemName::Shell,
            "changed",
            json!({ "ok": true }),
        )
        .await
        .unwrap();

        let req = rx.recv().await.unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.from, SubsystemName::Shell);
        assert_eq!(req.to, SubsystemName::Agent);
        assert_eq!(req.path, "changed");
        assert!(matches!(req.reply, ReplyChannel::None));
    }

    #[tokio::test]
    async fn returns_error_for_missing_subsystem() {
        let bus = Bus::new();
        let err = bus
            .get(SubsystemName::Config, SubsystemName::Shell, "ping")
            .await
            .unwrap_err();
        assert_eq!(err, BusError::SubsystemNotFound(SubsystemName::Config));
    }

    #[tokio::test]
    async fn returns_error_when_subsystem_channel_is_closed() {
        let bus = Bus::new();
        let (tx, rx) = mpsc::channel(8);
        bus.register(SubsystemName::Config, tx).await;
        drop(rx);

        let err = bus
            .get(SubsystemName::Config, SubsystemName::Shell, "ping")
            .await
            .unwrap_err();
        assert_eq!(err, BusError::RequestChannelClosed(SubsystemName::Config));
    }
}
