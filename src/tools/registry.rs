use crate::tools;
use genai::chat::Tool;
use genai::chat::ToolCall;
use serde_json::{Value, json};
use std::path::Path;

pub fn tool_template(name: &str, description: &str, schema: Value) -> Tool {
    Tool {
        name: name.into(),
        description: Some(description.into()),
        schema: Some(schema),
        strict: Some(true),
        config: None,
    }
}

pub fn tools_registry() -> Vec<Tool> {
    vec![
        tools::fs::ls_tree(),
        tools::fs::read(),
        tools::fs::list(),
        tools::fs::stat(),
        tools::fs::write(),
        tools::fs::replace(),
        tools::fs::mkdir(),
        tools::fs::remove(),
        tools::fs::rename(),
        tools::fs::copy(),
        tools::shell::exec(),
    ]
}
pub fn dispatch_tool(run_root: &Path, tool_call: &ToolCall) -> String {
    match tool_call.fn_name.as_str() {
        "fs_read" => tools::fs::execute_read(run_root, &tool_call.fn_arguments),
        "fs_ls_tree" => tools::fs::execute_ls_tree(run_root, &tool_call.fn_arguments),
        "fs_list" => tools::fs::execute_list(run_root, &tool_call.fn_arguments),
        "fs_stat" => tools::fs::execute_stat(run_root, &tool_call.fn_arguments),
        "fs_write" => tools::fs::execute_write(run_root, &tool_call.fn_arguments),
        "fs_replace" => tools::fs::execute_replace(run_root, &tool_call.fn_arguments),
        "fs_mkdir" => tools::fs::execute_mkdir(run_root, &tool_call.fn_arguments),
        "fs_remove" => tools::fs::execute_remove(run_root, &tool_call.fn_arguments),
        "fs_rename" => tools::fs::execute_rename(run_root, &tool_call.fn_arguments),
        "fs_copy" => tools::fs::execute_copy(run_root, &tool_call.fn_arguments),
        "shell_exec" => tools::shell::execute(run_root, &tool_call.fn_arguments),
        name => json!({
            "status": "error",
            "error": format!("unknown tool: {name}"),
        })
        .to_string(),
    }
}
