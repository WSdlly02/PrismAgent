use crate::tools;
use genai::chat::ToolCall;
use serde_json::json;
use std::path::Path;

use genai::chat::Tool;
pub fn tools_registry() -> Vec<Tool> {
    // 添加工具到注册表
    vec![tools::fs::ls_tree(), tools::fs::read()]
}
pub fn dispatch_tool(run_root: &Path, tool_call: &ToolCall) -> String {
    match tool_call.fn_name.as_str() {
        "fs_read" => tools::fs::execute_read(run_root, &tool_call.fn_arguments),
        "fs_ls_tree" => tools::fs::execute_ls_tree(run_root, &tool_call.fn_arguments),
        name => json!({
            "status": "error",
            "error": format!("unknown tool: {name}"),
        })
        .to_string(),
    }
}
