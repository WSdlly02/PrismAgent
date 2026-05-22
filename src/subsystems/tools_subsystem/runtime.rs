use crate::bus::{
    Bus, Method, ReplyChannel, Request, Response, StreamChunk, Subsystem, SubsystemName,
};
use crate::subsystems::response_body_as;
use crate::subsystems::tools_subsystem::model::{
    AsyncToolExecutor, ToolCallDispatchRequest, ToolDispatchRequest, ToolFuture, ToolsSubsystem,
};
use crate::subsystems::tools_subsystem::{fs, shell, web};
use genai::chat::{Tool, ToolCall};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tokio::sync::mpsc;

pub fn tool_template(name: &str, description: &str, schema: Value) -> Tool {
    Tool {
        name: name.into(),
        description: Some(description.into()),
        schema: Some(schema),
        strict: Some(true),
        config: None,
    }
}

macro_rules! tool_entry {
    ($module:ident::$def_fn:ident / $exec_fn:ident) => {{
        fn execute(bus: Bus, run_root: PathBuf, args: Value) -> ToolFuture {
            Box::pin(async move { $module::$exec_fn(&bus, &run_root, &args).await })
        }
        ($module::$def_fn(), execute as AsyncToolExecutor)
    }};
}

macro_rules! register_tools {
    ($($name:expr => $module:ident::$def_fn:ident / $exec_fn:ident),* $(,)?) => {
        fn registered_tool_entries() -> Vec<(Tool, AsyncToolExecutor)> {
            vec![
                $(tool_entry!($module::$def_fn / $exec_fn),)*
            ]
        }
    };
}

register_tools! {
    "fs_ls_tree"  => fs::ls_tree  / execute_ls_tree,
    "fs_read"     => fs::read     / execute_read,
    "fs_list"     => fs::list     / execute_list,
    "fs_stat"     => fs::stat     / execute_stat,
    "fs_write"    => fs::write    / execute_write,
    "fs_replace"  => fs::replace  / execute_replace,
    "fs_mkdir"    => fs::mkdir    / execute_mkdir,
    "fs_remove"   => fs::remove   / execute_remove,
    "fs_rename"   => fs::rename   / execute_rename,
    "fs_copy"     => fs::copy     / execute_copy,
    "shell_exec"  => shell::exec  / execute,
    "web_search"  => web::search  / execute_search,
    "web_fetch"   => web::fetch   / execute_fetch,
}

impl ToolsSubsystem {
    pub fn new() -> Self {
        let mut tools = Vec::new();
        let mut tools_map = HashMap::new();

        for (tool, executor) in registered_tool_entries() {
            tools_map.insert(tool.name.to_string(), executor);
            tools.push(tool);
        }

        Self {
            tools: RwLock::new(tools),
            tools_map: RwLock::new(tools_map),
        }
    }

    pub fn tools_registry(&self) -> Vec<Tool> {
        self.tools.read().expect("tools registry poisoned").clone()
    }

    async fn dispatch_tool(
        &self,
        bus: Bus,
        run_root: PathBuf,
        name: &str,
        arguments: Value,
    ) -> String {
        let executor = {
            let map = self.tools_map.read().expect("tools map poisoned");
            map.get(name).copied()
        };

        match executor {
            Some(executor) => executor(bus, run_root, arguments).await,
            None => json!({
                "status": "error",
                "error": format!("unknown tool: {name}"),
            })
            .to_string(),
        }
    }

    pub async fn dispatch_tool_call(
        &self,
        bus: Bus,
        run_root: PathBuf,
        tool_call: &ToolCall,
    ) -> String {
        self.dispatch_tool(
            bus,
            run_root,
            tool_call.fn_name.as_ref(),
            tool_call.fn_arguments.clone(),
        )
        .await
    }

    async fn handle_request(&self, bus: Bus, req: &Request) -> Response {
        match (req.method, req.path.as_str()) {
            (Method::Get, "tools") => Response::ok(json!(self.tools_registry())),
            (Method::Post, "dispatch_tool") => {
                let request = match response_body_as::<ToolDispatchRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let output = self
                    .dispatch_tool(bus, request.run_root, &request.name, request.arguments)
                    .await;
                Response::ok(json!({ "output": output }))
            }
            (Method::Post, "dispatch_tool_call") => {
                let request = match response_body_as::<ToolCallDispatchRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let output = self
                    .dispatch_tool_call(bus, request.run_root, &request.tool_call)
                    .await;
                Response::ok(json!({ "output": output }))
            }
            _ => Response::not_found(req.path.as_str()),
        }
    }
}

pub fn tools_registry() -> Vec<Tool> {
    registered_tool_entries()
        .into_iter()
        .map(|(tool, _)| tool)
        .collect()
}

pub async fn dispatch_tool(bus: &Bus, run_root: &std::path::Path, tool_call: &ToolCall) -> String {
    let subsystem = ToolsSubsystem::new();
    subsystem
        .dispatch_tool_call(bus.clone(), run_root.to_path_buf(), tool_call)
        .await
}

impl Subsystem for ToolsSubsystem {
    fn name(&self) -> SubsystemName {
        SubsystemName::Tool
    }

    fn start(self, bus: Bus) -> mpsc::Sender<Request> {
        let (tx, mut rx) = mpsc::channel::<Request>(64);
        let subsystem = std::sync::Arc::new(self);

        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let subsystem = subsystem.clone();
                let bus = bus.clone();
                tokio::spawn(async move {
                    let response = subsystem.handle_request(bus, &req).await;
                    match req.reply {
                        ReplyChannel::Once(tx) => {
                            let _ = tx.send(response);
                        }
                        ReplyChannel::Stream(tx) => {
                            let _ = tx.send(StreamChunk::Delta(response.body)).await;
                            let _ = tx.send(StreamChunk::Done).await;
                        }
                        ReplyChannel::None => {
                            let _ = response;
                        }
                    }
                });
            }
        });

        tx
    }
}
