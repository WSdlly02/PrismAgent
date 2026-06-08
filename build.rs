use std::{env, fs, path::PathBuf};

const FALLBACK_INDEX: &str = r#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>PrismAgent</title></head>
<body><main><h1>PrismAgent</h1><p>Build web assets with npm run build in web/.</p></main></body>
</html>
"#;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let dist = manifest_dir.join("web").join("dist");
    let index = dist.join("index.html");

    println!("cargo:rerun-if-changed=web/dist");
    println!("cargo:rerun-if-changed=web/package.json");

    if !index.exists() {
        fs::create_dir_all(&dist).expect("create fallback web/dist");
        fs::write(index, FALLBACK_INDEX).expect("write fallback web/dist/index.html");
    }
}
