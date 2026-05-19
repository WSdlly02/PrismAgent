use genai::chat::Tool;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

pub fn ls_tree() -> Tool {
    Tool {
        name: "fs_ls_tree".into(),
        description: Some(
            "List the directory tree of a given path. Parameters: {\"path\": \"directory path\", \"depth\": 2}".into(),
        ),
        schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The path of the directory to list"},
                "depth": {"type": "integer", "description": "The depth of the directory tree to list"}
            },
            "required": ["path", "depth"]
        })),
        strict: Some(true),
        config: None,
    }
}
pub fn read() -> Tool {
    Tool {
        name: "fs_read".into(),
        description: Some(
            "Read a file from the filesystem. Parameters: {\"path\": \"file path\"}".into(),
        ),
        schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The path of the file to read"}
            },
            "required": ["path"]
        })),
        strict: Some(true),
        config: None,
    }
}

pub fn execute_read(run_root: &Path, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = resolve_tool_path(run_root, path);
    match fs::read_to_string(&path) {
        Ok(content) => json!({
            "status": "ok",
            "path": path.display().to_string(),
            "content": content,
        })
        .to_string(),
        Err(error) => json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": error.to_string(),
        })
        .to_string(),
    }
}

pub fn execute_ls_tree(run_root: &Path, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(2) as usize;
    let path = resolve_tool_path(run_root, path);
    let mut entries = Vec::new();
    match collect_tree_entries(&path, depth, 0, &mut entries) {
        Ok(()) => json!({
            "status": "ok",
            "path": path.display().to_string(),
            "entries": entries,
        })
        .to_string(),
        Err(error) => json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": error.to_string(),
        })
        .to_string(),
    }
}

fn collect_tree_entries(
    path: &Path,
    max_depth: usize,
    current_depth: usize,
    entries: &mut Vec<String>,
) -> anyhow::Result<()> {
    if current_depth > max_depth {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let kind = if entry_path.is_dir() { "dir" } else { "file" };
        let indent = "  ".repeat(current_depth);
        entries.push(format!(
            "{indent}[{kind}] {}",
            entry.file_name().to_string_lossy()
        ));
        if entry_path.is_dir() && current_depth < max_depth {
            collect_tree_entries(&entry_path, max_depth, current_depth + 1, entries)?;
        }
    }
    Ok(())
}

fn resolve_tool_path(run_root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    run_root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|workspace_dir| workspace_dir.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}
