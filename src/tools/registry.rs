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

macro_rules! register_tools {
    ($($name:expr => $module:ident::$def_fn:ident / $exec_fn:ident),* $(,)?) => {
        pub fn tools_registry() -> Vec<Tool> {
            vec![
                $(tools::$module::$def_fn(),)*
            ]
        }

        pub async fn dispatch_tool(run_root: &Path, tool_call: &ToolCall) -> String {
            match tool_call.fn_name.as_str() {
                $($name => tools::$module::$exec_fn(run_root, &tool_call.fn_arguments).await,)*
                name => json!({
                    "status": "error",
                    "error": format!("unknown tool: {name}"),
                })
                .to_string(),
            }
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
