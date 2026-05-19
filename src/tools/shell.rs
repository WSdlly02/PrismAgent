use crate::tools::fs::resolve_tool_path;
use crate::tools::registry::tool_template;
use genai::chat::Tool;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::time::{Duration, Instant};

pub fn exec() -> Tool {
    tool_template(
        "shell_exec",
        "Execute a shell command in the workspace. Use for build/test/search commands.",
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

pub async fn execute(run_root: &std::path::Path, args: &Value) -> String {
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

    let cwd = resolve_tool_path(run_root, cwd);
    let mut child = match Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(&cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return json!({
                "status": "error",
                "command": command,
                "cwd": cwd.display().to_string(),
                "error": error.to_string(),
            })
            .to_string();
        }
    };

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let timed_out = tokio::select! {
        result = child.wait() => {
            match result {
                Ok(_) => false,
                Err(error) => {
                    return json!({
                        "status": "error",
                        "command": command,
                        "cwd": cwd.display().to_string(),
                        "error": error.to_string(),
                    })
                    .to_string();
                }
            }
        }
        _ = tokio::time::sleep_until(deadline) => {
            let _ = child.kill().await;
            true
        }
    };

    match child.wait_with_output().await {
        Ok(output) => if timed_out {
            json!({
                "status": "timeout",
                "command": command,
                "cwd": cwd.display().to_string(),
                "timeout_secs": timeout_secs,
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            })
        } else {
            json!({
                "status": "ok",
                "command": command,
                "cwd": cwd.display().to_string(),
                "exit_code": output.status.code(),
                "success": output.status.success(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            })
        }
        .to_string(),
        Err(error) => json!({
            "status": if timed_out { "timeout" } else { "error" },
            "command": command,
            "cwd": cwd.display().to_string(),
            "error": error.to_string(),
        })
        .to_string(),
    }
}
