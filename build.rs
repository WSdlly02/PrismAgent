use std::{env, fs, path::PathBuf};

const FALLBACK_INDEX: &str = r#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>PrismAgent</title></head>
<body><main><h1>PrismAgent</h1><p>Build web assets with npm run build in web/.</p></main></body>
</html>
"#;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));

    // ---- web 前端资源 ----
    let dist = manifest_dir.join("web").join("dist");
    let index = dist.join("index.html");

    println!("cargo:rerun-if-changed=web/dist");
    println!("cargo:rerun-if-changed=web/package.json");

    if !index.exists() {
        fs::create_dir_all(&dist).expect("create fallback web/dist");
        fs::write(index, FALLBACK_INDEX).expect("write fallback web/dist/index.html");
        println!(
            "cargo:warning=web/dist/index.html was missing; wrote fallback page. Run npm run build in web/ before packaging prismagentd."
        );
    } else if fs::read_to_string(&index)
        .map(|content| content.contains("Build web assets with npm run build in web/."))
        .unwrap_or(false)
    {
        println!(
            "cargo:warning=web/dist/index.html is the fallback page. Run npm run build in web/ to embed the real UI."
        );
    }

    // ---- stdlib 资源（profiles + skills）----
    // include_str! 已确保 cargo 跟踪这些文件的变更，
    // 这里显式声明以强化构建依赖关系。
    let stdlib_profiles = manifest_dir.join("stdlib").join("profiles");
    let stdlib_skills = manifest_dir.join("stdlib").join("skills");

    if stdlib_profiles.exists() {
        for entry in fs::read_dir(&stdlib_profiles).expect("read stdlib/profiles") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    } else {
        println!(
            "cargo:warning=stdlib/profiles directory not found at {}",
            stdlib_profiles.display()
        );
    }

    if stdlib_skills.exists() {
        for entry in fs::read_dir(&stdlib_skills).expect("read stdlib/skills") {
            let skill_dir = entry.expect("entry").path();
            let skill_md = skill_dir.join("SKILL.md");
            if skill_md.exists() {
                println!("cargo:rerun-if-changed={}", skill_md.display());
            }
        }
    } else {
        println!(
            "cargo:warning=stdlib/skills directory not found at {}",
            stdlib_skills.display()
        );
    }
}
