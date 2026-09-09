use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::actors::tools_actor::provider::{ToolResult, ToolSpec};
use reqwest::{Client, Response};
use serde_json::{Value, json};

static CLIENT: std::sync::LazyLock<Client> = std::sync::LazyLock::new(Client::new);

async fn api_key(_ctx: &ToolExecutionContext) -> Result<String, &'static str> {
    let key = std::env::var("TINYFISH_API_KEY").map_err(|_| "TINYFISH_API_KEY is missing")?;
    if key.trim().is_empty() {
        Err("TINYFISH_API_KEY is empty")
    } else {
        Ok(key)
    }
}

// ─── web_search ───────────────────────────────────────────────────────────────

pub fn search() -> ToolSpec {
    ToolSpec::new(
        "web_search",
        "Use TinyFish to search the internet for real-time information, returning a list of titles/summaries/URLs. Use when you don't know the target URL.",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "language": { "type": "string", "description": "Language code like zh/en, optional" }
            },
            "required": ["query"]
        }),
    )
}

pub async fn execute_search(ctx: ToolExecutionContext, args: Value) -> ToolResult {
    let query = match args["query"].as_str() {
        Some(q) => q,
        None => return ToolResult::error(json!({"error": "missing query"})),
    };
    let api_key = match api_key(&ctx).await {
        Ok(api_key) => api_key,
        Err(error) => return ToolResult::error(json!({"error": error})),
    };

    let mut url = format!(
        "https://api.search.tinyfish.ai?query={}",
        urlencoding::encode(query)
    );
    if let Some(lang) = args["language"].as_str() {
        url.push_str(&format!("&language={}", lang));
    }

    match CLIENT.get(&url).header("X-API-Key", api_key).send().await {
        Ok(response) => response_result(response).await,
        Err(error) => ToolResult::error(json!({"error": error.to_string()})),
    }
}

// ─── web_fetch ────────────────────────────────────────────────────────────────

pub fn fetch() -> ToolSpec {
    ToolSpec::new(
        "web_fetch",
        "Fetch the main content of specified URLs, supporting JS-rendered pages. Use when the target URL is known, up to 10 URLs.",
        json!({
            "type": "object",
            "properties": {
                "urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of URLs to fetch, up to 10"
                }
            },
            "required": ["urls"]
        }),
    )
}

pub async fn execute_fetch(ctx: ToolExecutionContext, args: Value) -> ToolResult {
    let urls = match args["urls"].as_array() {
        Some(u) => u.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
        None => return ToolResult::error(json!({"error": "missing urls"})),
    };
    let api_key = match api_key(&ctx).await {
        Ok(api_key) => api_key,
        Err(error) => return ToolResult::error(json!({"error": error})),
    };

    let body = json!({ "urls": urls, "format": "markdown" });

    match CLIENT
        .post("https://api.fetch.tinyfish.ai")
        .header("X-API-Key", api_key)
        .json(&body)
        .send()
        .await
    {
        Ok(response) => response_result(response).await,
        Err(error) => ToolResult::error(json!({"error": error.to_string()})),
    }
}

async fn response_result(response: Response) -> ToolResult {
    let status = response.status();
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => return ToolResult::error(json!({"error": error.to_string()})),
    };
    let content = serde_json::from_str(&text).unwrap_or(Value::String(text));
    if status.is_success() {
        ToolResult::success(content)
    } else {
        ToolResult::error(json!({
            "http_status": status.as_u16(),
            "body": content,
        }))
    }
}
