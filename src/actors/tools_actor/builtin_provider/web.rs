use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::actors::tools_actor::provider::{ToolResult, ToolSpec};
use reqwest::{Client, Response};
use serde_json::{Value, json};

static CLIENT: std::sync::LazyLock<Client> = std::sync::LazyLock::new(Client::new);

async fn api_key(_ctx: &ToolExecutionContext) -> Result<String, String> {
    let key =
        std::env::var("TINYFISH_API_KEY").map_err(|_| "TINYFISH_API_KEY is missing".to_string())?;
    if key.trim().is_empty() {
        Err("TINYFISH_API_KEY is empty".to_string())
    } else {
        Ok(key)
    }
}

// ─── web_search ───────────────────────────────────────────────────────────────

pub fn search() -> ToolSpec {
    ToolSpec::new(
        "web_search",
        "使用 TinyFish 搜索互联网实时信息，返回标题/摘要/URL列表。不知道目标URL时使用。",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "搜索关键词" },
                "language": { "type": "string", "description": "语言代码如 zh/en，可选" }
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
        "抓取指定URL的网页正文内容，支持JS渲染页面。已知URL时使用，最多10个URL。",
        json!({
            "type": "object",
            "properties": {
                "urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "要抓取的URL列表，最多10个"
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
