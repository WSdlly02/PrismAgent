use crate::tools;
use genai::chat::Tool;
pub fn tools_registry() -> Vec<Tool> {
    let mut tools: Vec<Tool> = Vec::new();
    tools.push(tools::fs::ls_tree());
    tools.push(tools::fs::read());
    tools
}
pub async fn tool_router() {}
