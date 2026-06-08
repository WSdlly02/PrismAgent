use axum::{
    body::Body,
    http::{Response, StatusCode, header},
};
use include_dir::{Dir, include_dir};

static WEB_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

pub struct EmbeddedAsset {
    pub bytes: &'static [u8],
    pub content_type: &'static str,
}

pub fn embedded_asset(path: &str) -> Option<EmbeddedAsset> {
    let normalized = normalize_path(path);
    if normalized.starts_with("api/") || normalized == "api" {
        return None;
    }
    if let Some(file) = WEB_DIST.get_file(&normalized) {
        return Some(EmbeddedAsset {
            bytes: file.contents(),
            content_type: content_type(&normalized),
        });
    }
    if normalized.starts_with("assets/") {
        return None;
    }
    WEB_DIST.get_file("index.html").map(|file| EmbeddedAsset {
        bytes: file.contents(),
        content_type: "text/html; charset=utf-8",
    })
}

pub fn asset_response(path: &str) -> Response<Body> {
    match embedded_asset(path) {
        Some(asset) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, asset.content_type)
            .body(Body::from(asset.bytes))
            .expect("static asset response is valid"),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("not found response is valid"),
    }
}

fn normalize_path(path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        "index.html".to_string()
    } else {
        path.to_string()
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_resolves_to_index_html() {
        let asset = embedded_asset("/").expect("index asset");

        assert_eq!(asset.content_type, "text/html; charset=utf-8");
        assert!(asset.bytes.starts_with(b"<!doctype html>"));
    }

    #[test]
    fn unknown_routes_fall_back_to_index_html() {
        let asset = embedded_asset("/workspaces/example").expect("spa fallback");

        assert_eq!(asset.content_type, "text/html; charset=utf-8");
        assert!(asset.bytes.starts_with(b"<!doctype html>"));
    }

    #[test]
    fn missing_asset_paths_do_not_fall_back() {
        assert!(embedded_asset("/assets/not-found.js").is_none());
    }
}
