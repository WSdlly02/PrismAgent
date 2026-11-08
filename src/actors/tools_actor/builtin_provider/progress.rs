use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::actors::tools_actor::provider::{ToolResult, ToolSpec};
use serde_json::{Value, json};

pub fn publish() -> ToolSpec {
    ToolSpec::new(
        "pub",
        "Optionally publish a concise progress update to the user. Use command for a user-facing description of your current action or next step, not private reasoning. This tool only displays text; it does not execute commands or mark the task complete.",
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Progress message to display to the user; Markdown is supported"}
            },
            "required": ["command"],
            "additionalProperties": false
        }),
    )
}

pub async fn execute_publish(_ctx: ToolExecutionContext, args: Value) -> ToolResult {
    acknowledge(&args)
}

fn acknowledge(args: &Value) -> ToolResult {
    // The schema constrains model output, but execution tolerates extra fields.
    // Arguments remain in the persisted call; only command controls presentation.
    match args.get("command").and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => {
            // The persisted call carries the display text; no second message is emitted.
            ToolResult::success(json!({"published": true}))
        }
        _ => ToolResult::error(json!({"error": "command must be a non-empty string"})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::storage_actor::model::unit::Unit;
    use genai::chat::{ChatMessage, ToolCall};

    #[test]
    fn extra_arguments_are_accepted_and_preserved_in_unit_serialization() {
        let spec = publish();
        assert_eq!(spec.strict, Some(true));
        assert_eq!(spec.input_schema["additionalProperties"], json!(false));
        let args = json!({"command": "Check workspace and SSH", "depth": 2, "path": "."});
        assert!(!acknowledge(&args).is_error);
        let call = ToolCall {
            call_id: "pub-test".into(),
            fn_name: "pub".into(),
            fn_arguments: args.clone(),
            thought_signatures: None,
        };
        let unit = Unit::from_chat_message(ChatMessage::assistant(vec![call]));
        let serialized = serde_json::to_string(&unit).expect("serialize unit");
        let restored: Unit = serde_json::from_str(&serialized).expect("deserialize unit");
        assert_eq!(restored.content.content.tool_calls()[0].fn_arguments, args);
    }

    #[test]
    fn validates_progress_text_without_executing_it() {
        assert!(!acknowledge(&json!({"command": "Check workspace and SSH"})).is_error);
        for args in [json!({}), json!({"command": 1}), json!({"command": "  "})] {
            assert!(acknowledge(&args).is_error);
        }
    }
}
