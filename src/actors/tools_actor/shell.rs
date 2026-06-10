use crate::actors::tools_actor::fs::{resolve_tool_path, run_rtk_json};
use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::actors::tools_actor::runtime::tool_template;
use genai::chat::Tool;
use serde_json::{Value, json};

pub fn exec() -> Tool {
    tool_template(
        "shell_exec",
        "Execute a shell command in the workspace through rtk. Known commands are token-optimized; unknown commands are passed through.",
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to execute"},
                "cwd": {"type": "string", "description": "Working directory relative to workspace, or absolute path"},
                "timeout_secs": {"type": "integer", "description": "Timeout in seconds, default 30, max 300"}
            },
            "required": ["command", "cwd", "timeout_secs"]
        }),
    )
}

pub async fn execute(ctx: ToolExecutionContext, args: Value) -> String {
    let command = args.get("command").and_then(Value::as_str).unwrap_or("");
    let cwd = args.get("cwd").and_then(Value::as_str).unwrap_or(".");
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(1, 300);

    if command.trim().is_empty() {
        return json!({
            "status": "error",
            "error": "command must not be empty",
        })
        .to_string();
    }

    let cwd = resolve_tool_path(&ctx.workspace_path, cwd);
    let rtk_args = rtk_args_for_shell_command(command);
    run_rtk_json(&cwd, rtk_args, None, timeout_secs).await
}

fn rtk_args_for_shell_command(command: &str) -> Vec<String> {
    if is_simple_command(command) {
        command
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>()
    } else {
        vec!["run".to_string(), "-c".to_string(), command.to_string()]
    }
}

fn is_simple_command(command: &str) -> bool {
    !command.is_empty()
        && !command.chars().any(|ch| {
            matches!(
                ch,
                '|' | '&' | ';' | '<' | '>' | '(' | ')' | '$' | '`' | '\'' | '"' | '\\'
            )
        })
}
