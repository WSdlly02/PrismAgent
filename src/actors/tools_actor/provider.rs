use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::error::{SubsystemError, SubsystemResult};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type ToolFuture = Pin<Box<dyn Future<Output = SubsystemResult<ToolResult>> + Send>>;
pub type ToolExecutor = fn(ToolExecutionContext, Value) -> ToolFuture;

/// Provider-neutral result. `content` is always JSON; `is_error` distinguishes
/// tool execution failures from successful results. Provider/protocol failures
/// remain `SubsystemError`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub content: Value,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(content: Value) -> Self {
        Self {
            content,
            is_error: false,
        }
    }

    pub fn error(content: Value) -> Self {
        Self {
            content,
            is_error: true,
        }
    }

    pub fn into_response_content(self) -> String {
        serde_json::json!({
            "is_error": self.is_error,
            "content": self.content,
        })
        .to_string()
    }
}

/// Provider-neutral description of a tool exposed to an agent.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub strict: Option<bool>,
}

impl ToolSpec {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            input_schema,
            output_schema: None,
            strict: Some(true),
        }
    }
}

/// Execution boundary shared by builtin tools and external tool protocols.
pub trait ToolProvider: Send + Sync {
    fn id(&self) -> &str;

    fn tools(&self) -> &[ToolSpec];

    fn call(&self, tool_name: String, ctx: ToolExecutionContext, arguments: Value) -> ToolFuture;
}

/// Immutable routing table built from all providers during runtime startup.
pub struct ToolRouter {
    tools: Vec<ToolSpec>,
    routes: HashMap<String, Arc<dyn ToolProvider>>, // tool name -> provider
}

impl ToolRouter {
    pub fn new(providers: Vec<Arc<dyn ToolProvider>>) -> SubsystemResult<Self> {
        let tool_count = providers
            .iter()
            .map(|provider| provider.tools().len())
            .sum();
        let mut tools = Vec::with_capacity(tool_count);
        let mut routes = HashMap::with_capacity(tool_count);
        let mut provider_ids = HashSet::with_capacity(providers.len());

        for provider in providers {
            let provider_id = provider.id();
            if provider_id.is_empty() {
                return Err(SubsystemError::configuration(
                    "tools",
                    "tool provider id must not be empty",
                ));
            }
            if provider_id.trim() != provider_id {
                return Err(SubsystemError::configuration(
                    "tools",
                    format!("tool provider id has surrounding whitespace: {provider_id:?}"),
                ));
            }
            if !provider_ids.insert(provider_id.to_string()) {
                return Err(SubsystemError::configuration(
                    "tools",
                    format!("duplicate tool provider id: {provider_id}"),
                ));
            }

            for tool in provider.tools() {
                let tool_name = tool.name.as_str();
                if tool_name.is_empty() {
                    return Err(SubsystemError::configuration(
                        "tools",
                        format!("provider {provider_id} registered an empty tool name"),
                    ));
                }
                if tool_name.trim() != tool_name {
                    return Err(SubsystemError::configuration(
                        "tools",
                        format!(
                            "provider {provider_id} registered a tool name with surrounding whitespace: {tool_name:?}"
                        ),
                    ));
                }
                if routes
                    .insert(tool_name.to_string(), provider.clone())
                    .is_some()
                {
                    return Err(SubsystemError::configuration(
                        "tools",
                        format!("duplicate tool name: {tool_name}"),
                    ));
                }
                tools.push(tool.clone());
            }
        }

        Ok(Self { tools, routes })
    }

    pub fn resolve(&self, names: Option<&[String]>) -> Vec<&ToolSpec> {
        let all_names = names.is_none_or(|names| names.iter().any(|name| name == "*"));
        let allowed_names =
            names.map(|names| names.iter().map(String::as_str).collect::<HashSet<_>>());
        self.tools
            .iter()
            .filter(|tool| {
                all_names
                    || allowed_names
                        .as_ref()
                        .is_some_and(|names| names.contains(tool.name.as_str()))
            })
            .collect()
    }

    pub fn call(
        &self,
        tool_name: String,
        ctx: ToolExecutionContext,
        arguments: Value,
    ) -> ToolFuture {
        let Some(provider) = self.routes.get(&tool_name) else {
            return Box::pin(async move {
                Err(SubsystemError::validation(format!(
                    "unknown tool: {tool_name}"
                )))
            });
        };
        provider.call(tool_name, ctx, arguments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handles::test_handles;
    use std::path::PathBuf;

    struct StubProvider {
        id: &'static str,
        tools: Vec<ToolSpec>,
    }

    impl ToolProvider for StubProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn tools(&self) -> &[ToolSpec] {
            &self.tools
        }

        fn call(
            &self,
            _tool_name: String,
            _ctx: ToolExecutionContext,
            _arguments: Value,
        ) -> ToolFuture {
            Box::pin(async { Ok(ToolResult::success(serde_json::json!({}))) })
        }
    }

    fn stub(id: &'static str, tool_name: &str) -> Arc<dyn ToolProvider> {
        Arc::new(StubProvider {
            id,
            tools: vec![ToolSpec::new(tool_name, "stub", serde_json::json!({}))],
        })
    }

    #[test]
    fn router_rejects_duplicate_tool_names_across_providers() {
        let error = match ToolRouter::new(vec![stub("first", "same"), stub("second", "same")]) {
            Ok(_) => panic!("duplicate tool names must be rejected"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("duplicate tool name: same"));
    }

    #[test]
    fn tool_result_serializes_error_state_and_json_content() {
        let content =
            ToolResult::error(serde_json::json!({"error": "failed"})).into_response_content();

        assert_eq!(
            serde_json::from_str::<Value>(&content).expect("tool result JSON"),
            serde_json::json!({
                "is_error": true,
                "content": {"error": "failed"},
            })
        );
    }

    #[test]
    fn resolve_filters_registered_tools_by_name() {
        let router = ToolRouter::new(vec![
            stub("builtin", "file_read"),
            stub("other", "web_search"),
        ])
        .expect("router");

        let tools = router.resolve(Some(&["web_search".to_string()]));
        let names = tools
            .into_iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["web_search"]);
    }

    #[test]
    fn resolve_wildcard_includes_all_registered_tools() {
        let router = ToolRouter::new(vec![
            stub("builtin", "file_read"),
            stub("other", "web_search"),
        ])
        .expect("router");

        assert_eq!(router.resolve(Some(&["*".to_string()])).len(), 2);
    }

    #[tokio::test]
    async fn call_routes_to_the_owning_provider() {
        let router = ToolRouter::new(vec![stub("builtin", "file_read")]).expect("router");

        let result = router
            .call(
                "file_read".to_string(),
                ToolExecutionContext {
                    handles: test_handles(),
                    workspace_uuid: "workspace".to_string(),
                    caller_agent_uuid: "agent".to_string(),
                    workspace_path: PathBuf::from("/tmp"),
                },
                serde_json::json!({}),
            )
            .await
            .expect("provider call");

        assert_eq!(result, ToolResult::success(serde_json::json!({})));
    }
}
