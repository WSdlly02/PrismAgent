use std::{fs, path::PathBuf};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use genai::{
    ClientBuilder, ServiceTarget, WebConfig,
    adapter::AdapterKind,
    resolver::{AuthData, AuthResolver, Endpoint},
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

#[derive(serde::Deserialize)]
struct CodexAuthFile {
    tokens: CodexTokens,
}

#[derive(serde::Deserialize)]
struct CodexTokens {
    access_token: String,
    refresh_token: Option<String>,
}

fn codex_auth_path() -> anyhow::Result<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(home).join("auth.json"));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法找到 home 目录"))?;
    Ok(home.join(".codex").join("auth.json"))
}

fn load_codex_access_token() -> anyhow::Result<String> {
    let path = codex_auth_path()?;
    let content = fs::read_to_string(path)?;
    let auth: CodexAuthFile = serde_json::from_str(&content)?;
    Ok(auth.tokens.access_token)
}

fn extract_chatgpt_account_id(jwt: &str) -> anyhow::Result<String> {
    let payload = jwt
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("非法 JWT：缺少 payload"))?;

    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| anyhow::anyhow!("JWT payload base64url 解码失败: {e}"))?;

    let claims: serde_json::Value = serde_json::from_slice(&decoded)?;

    for key in ["chatgpt_account_id", "sub"] {
        if let Some(v) = claims.get(key).and_then(|v| v.as_str()) {
            return Ok(v.to_string());
        }
    }

    if let Some(v) = claims
        .pointer("/https://api.openai.com/profile/id")
        .and_then(|v| v.as_str())
    {
        return Ok(v.to_string());
    }

    anyhow::bail!("无法从 Codex access_token 提取 chatgpt-account-id")
}

pub fn build_codex_oauth_client(builder: ClientBuilder) -> genai::Client {
    let access_token = load_codex_access_token().expect("读取 ~/.codex/auth.json 失败");
    let account_id =
        extract_chatgpt_account_id(&access_token).expect("提取 chatgpt-account-id 失败");

    let mut default_headers = HeaderMap::new();
    default_headers.insert(
        HeaderName::from_static("chatgpt-account-id"),
        HeaderValue::from_str(&account_id).expect("非法 chatgpt-account-id"),
    );

    let web_config = WebConfig::default().with_default_headers(default_headers);

    builder
        .with_adapter_kind(AdapterKind::OpenAIResp)
        .with_auth_resolver(AuthResolver::from_resolver_fn(move |_| {
            Ok(Some(AuthData::from_single(access_token.clone())))
        }))
        .with_service_target_resolver_fn(|mut target: ServiceTarget| {
            target.endpoint = Endpoint::from_static("https://chatgpt.com/backend-api/codex/");
            Ok(target)
        })
        .with_web_config(web_config)
        .build()
}
