use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::actors::tools_actor::runtime::tool_template;
use genai::chat::Tool;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

pub fn ls_tree() -> Tool {
    tool_template(
        "fs_ls_tree",
        "List the directory tree of a given path.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The path of the directory to list"},
                "depth": {"type": "integer", "description": "The depth of the directory tree to list"}
            },
            "required": ["path", "depth"]
        }),
    )
}

pub fn read() -> Tool {
    tool_template(
        "fs_read",
        "Read a UTF-8 text file from the filesystem.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The path of the file to read"}
            },
            "required": ["path"]
        }),
    )
}

pub fn list() -> Tool {
    tool_template(
        "fs_list",
        "List direct children of a directory.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path"}
            },
            "required": ["path"]
        }),
    )
}

pub fn stat() -> Tool {
    tool_template(
        "fs_stat",
        "Return metadata for a file or directory.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File or directory path"}
            },
            "required": ["path"]
        }),
    )
}

pub fn write() -> Tool {
    tool_template(
        "fs_write",
        "Write a UTF-8 text file. Creates the file if it does not exist.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "content": {"type": "string", "description": "New file content"},
                "create_parent_dirs": {"type": "boolean", "description": "Create missing parent directories"}
            },
            "required": ["path", "content", "create_parent_dirs"]
        }),
    )
}

pub fn replace() -> Tool {
    tool_template(
        "fs_replace",
        "Replace one exact text occurrence in a UTF-8 text file.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "old": {"type": "string", "description": "Exact text to replace"},
                "new": {"type": "string", "description": "Replacement text"}
            },
            "required": ["path", "old", "new"]
        }),
    )
}

pub fn mkdir() -> Tool {
    tool_template(
        "fs_mkdir",
        "Create a directory and any missing parent directories.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path"}
            },
            "required": ["path"]
        }),
    )
}

pub fn remove() -> Tool {
    tool_template(
        "fs_remove",
        "Remove a file or an empty directory. Recursive removal requires recursive=true.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to remove"},
                "recursive": {"type": "boolean", "description": "Recursively remove a directory"}
            },
            "required": ["path", "recursive"]
        }),
    )
}

pub fn rename() -> Tool {
    tool_template(
        "fs_rename",
        "Rename or move a file or directory.",
        json!({
            "type": "object",
            "properties": {
                "from": {"type": "string", "description": "Source path"},
                "to": {"type": "string", "description": "Destination path"}
            },
            "required": ["from", "to"]
        }),
    )
}

pub fn copy() -> Tool {
    tool_template(
        "fs_copy",
        "Copy a file.",
        json!({
            "type": "object",
            "properties": {
                "from": {"type": "string", "description": "Source file path"},
                "to": {"type": "string", "description": "Destination file path"}
            },
            "required": ["from", "to"]
        }),
    )
}

pub async fn execute_read(ctx: ToolExecutionContext, args: Value) -> String {
    let path = tool_arg(args.get("path").and_then(Value::as_str));
    run_rtk_json(
        &ctx.workspace_path,
        vec!["read".into(), path],
        None,
        DEFAULT_TIMEOUT_SECS,
    )
    .await
}

pub async fn execute_ls_tree(ctx: ToolExecutionContext, args: Value) -> String {
    let path = tool_arg(args.get("path").and_then(Value::as_str));
    let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(2);
    run_rtk_json(
        &ctx.workspace_path,
        vec!["tree".into(), "-L".into(), depth.to_string(), path],
        None,
        DEFAULT_TIMEOUT_SECS,
    )
    .await
}

pub async fn execute_list(ctx: ToolExecutionContext, args: Value) -> String {
    let path = tool_arg(args.get("path").and_then(Value::as_str));
    run_rtk_json(
        &ctx.workspace_path,
        vec!["ls".into(), "-la".into(), path],
        None,
        DEFAULT_TIMEOUT_SECS,
    )
    .await
}

pub async fn execute_stat(ctx: ToolExecutionContext, args: Value) -> String {
    let path = tool_arg(args.get("path").and_then(Value::as_str));
    run_rtk_json(
        &ctx.workspace_path,
        vec!["stat".into(), path],
        None,
        DEFAULT_TIMEOUT_SECS,
    )
    .await
}

pub async fn execute_write(ctx: ToolExecutionContext, args: Value) -> String {
    let path_arg = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = resolve_tool_path(&ctx.workspace_path, path_arg);
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let create_parent_dirs = args
        .get("create_parent_dirs")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if create_parent_dirs
        && let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": error.to_string(),
        })
        .to_string();
    }
    match fs::write(&path, content) {
        Ok(()) => json!({
            "status": "ok",
            "path": path.display().to_string(),
            "bytes": content.len(),
        })
        .to_string(),
        Err(error) => json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": error.to_string(),
        })
        .to_string(),
    }
}

pub async fn execute_replace(ctx: ToolExecutionContext, args: Value) -> String {
    let path_arg = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = resolve_tool_path(&ctx.workspace_path, path_arg);
    let old = args.get("old").and_then(Value::as_str).unwrap_or("");
    let new = args.get("new").and_then(Value::as_str).unwrap_or("");
    if old.is_empty() {
        return json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": "old must not be empty",
        })
        .to_string();
    }
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            return json!({
                "status": "error",
                "path": path.display().to_string(),
                "error": error.to_string(),
            })
            .to_string();
        }
    };
    let count = content.matches(old).count();
    if count != 1 {
        return json!({
            "status": "error",
            "path": path.display().to_string(),
            "matches": count,
            "error": "old text must match exactly once",
        })
        .to_string();
    }
    let updated = content.replacen(old, new, 1);
    match fs::write(&path, updated) {
        Ok(()) => json!({
            "status": "ok",
            "path": path.display().to_string(),
            "replacements": 1,
        })
        .to_string(),
        Err(error) => json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": error.to_string(),
        })
        .to_string(),
    }
}

pub async fn execute_mkdir(ctx: ToolExecutionContext, args: Value) -> String {
    let path = tool_arg(args.get("path").and_then(Value::as_str));
    run_rtk_json(
        &ctx.workspace_path,
        vec!["mkdir".into(), "-p".into(), path],
        None,
        DEFAULT_TIMEOUT_SECS,
    )
    .await
}

pub async fn execute_remove(ctx: ToolExecutionContext, args: Value) -> String {
    let path = tool_arg(args.get("path").and_then(Value::as_str));
    let recursive = args
        .get("recursive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut command = vec!["rm".into()];
    if recursive {
        command.push("-rf".into());
    } else {
        command.push("-f".into());
    }
    command.push(path);
    run_rtk_json(&ctx.workspace_path, command, None, DEFAULT_TIMEOUT_SECS).await
}

pub async fn execute_rename(ctx: ToolExecutionContext, args: Value) -> String {
    let from = tool_arg(args.get("from").and_then(Value::as_str));
    let to = tool_arg(args.get("to").and_then(Value::as_str));
    run_rtk_json(
        &ctx.workspace_path,
        vec!["mv".into(), from, to],
        None,
        DEFAULT_TIMEOUT_SECS,
    )
    .await
}

pub async fn execute_copy(ctx: ToolExecutionContext, args: Value) -> String {
    let from = tool_arg(args.get("from").and_then(Value::as_str));
    let to = tool_arg(args.get("to").and_then(Value::as_str));
    run_rtk_json(
        &ctx.workspace_path,
        vec!["cp".into(), from, to],
        None,
        DEFAULT_TIMEOUT_SECS,
    )
    .await
}

pub(super) async fn run_rtk_json(
    cwd: &Path,
    args: Vec<String>,
    stdin: Option<Vec<u8>>,
    timeout_secs: u64,
) -> String {
    let command = format!("rtk {}", args.join(" "));
    let mut child = match Command::new("rtk")
        .args(&args)
        .current_dir(cwd)
        .kill_on_drop(true)
        .stdin(if stdin.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return json!({
                "status": "error",
                "command": command,
                "error": error.to_string(),
            })
            .to_string();
        }
    };
    if let Some(input) = stdin
        && let Some(mut child_stdin) = child.stdin.take()
    {
        tokio::spawn(async move {
            let _ = child_stdin.write_all(&input).await;
        });
    }
    match tokio::time::timeout(
        Duration::from_secs(timeout_secs.clamp(1, 300)),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => json!({
            "status": if output.status.success() { "ok" } else { "error" },
            "command": command,
            "exit_code": output.status.code(),
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        })
        .to_string(),
        Ok(Err(error)) => json!({
            "status": "error",
            "command": command,
            "error": error.to_string(),
        })
        .to_string(),
        Err(_) => json!({
            "status": "timeout",
            "command": command,
            "timeout_secs": timeout_secs,
        })
        .to_string(),
    }
}

pub(super) fn resolve_tool_path(workspace_path: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    workspace_path.join(path)
}

fn tool_arg(path: Option<&str>) -> String {
    path.unwrap_or(".").to_string()
}
