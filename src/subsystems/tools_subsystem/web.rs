use crate::bus::{Bus, SubsystemName};
use crate::subsystems::tools_subsystem::runtime::tool_template;
use genai::chat::Tool;
use reqwest::Client;
use serde_json::{Value, json};
use std::path::Path;

static CLIENT: std::sync::LazyLock<Client> = std::sync::LazyLock::new(Client::new);

async fn api_key(bus: &Bus) -> Result<String, String> {
    let response = bus
        .post(
            SubsystemName::Config,
            SubsystemName::Tools,
            "tool_config",
            json!({ "name": "tinyfish" }),
        )
        .await
        .map_err(|error| error.to_string())?;
    if !response.is_ok() {
        return Err(response.body.to_string());
    }
    response
        .body
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| "tools.tinyfish.api_key is missing".to_string())
}

// ─── web_search ───────────────────────────────────────────────────────────────

pub fn search() -> Tool {
    tool_template(
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

pub async fn execute_search(bus: &Bus, _run_root: &Path, args: &Value) -> String {
    let query = match args["query"].as_str() {
        Some(q) => q,
        None => return json!({"status":"error","error":"missing query"}).to_string(),
    };
    let api_key = match api_key(bus).await {
        Ok(api_key) => api_key,
        Err(error) => return json!({"status":"error","error":error}).to_string(),
    };

    let mut url = format!(
        "https://api.search.tinyfish.ai?query={}",
        urlencoding::encode(query)
    );
    if let Some(lang) = args["language"].as_str() {
        url.push_str(&format!("&language={}", lang));
    }

    match CLIENT.get(&url).header("X-API-Key", api_key).send().await {
        Ok(resp) => resp
            .text()
            .await
            .unwrap_or_else(|e| json!({"status":"error","error":e.to_string()}).to_string()),
        Err(e) => json!({"status":"error","error":e.to_string()}).to_string(),
    }
}

// ─── web_fetch ────────────────────────────────────────────────────────────────

pub fn fetch() -> Tool {
    tool_template(
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

pub async fn execute_fetch(bus: &Bus, _run_root: &Path, args: &Value) -> String {
    let urls = match args["urls"].as_array() {
        Some(u) => u.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>(),
        None => return json!({"status":"error","error":"missing urls"}).to_string(),
    };
    let api_key = match api_key(bus).await {
        Ok(api_key) => api_key,
        Err(error) => return json!({"status":"error","error":error}).to_string(),
    };

    let body = json!({ "urls": urls, "format": "markdown" });

    match CLIENT
        .post("https://api.fetch.tinyfish.ai")
        .header("X-API-Key", api_key)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => resp
            .text()
            .await
            .unwrap_or_else(|e| json!({"status":"error","error":e.to_string()}).to_string()),
        Err(e) => json!({"status":"error","error":e.to_string()}).to_string(),
    }
}
