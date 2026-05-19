use crate::tools::registry::tool_template;
use genai::chat::Tool;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

pub fn ls_tree() -> Tool {
    tool_template(
        "fs_ls_tree",
        "List the directory tree of a given path.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The path of the directory to list"},
                "depth": {"type": "integer", "description": "The depth of the directory tree to list"}
            },
            "required": ["path", "depth"]
        }),
    )
}

pub fn read() -> Tool {
    tool_template(
        "fs_read",
        "Read a UTF-8 text file from the filesystem.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The path of the file to read"}
            },
            "required": ["path"]
        }),
    )
}

pub fn list() -> Tool {
    tool_template(
        "fs_list",
        "List direct children of a directory.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path"}
            },
            "required": ["path"]
        }),
    )
}

pub fn stat() -> Tool {
    tool_template(
        "fs_stat",
        "Return metadata for a file or directory.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File or directory path"}
            },
            "required": ["path"]
        }),
    )
}

pub fn write() -> Tool {
    tool_template(
        "fs_write",
        "Write a UTF-8 text file. Creates the file if it does not exist.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "content": {"type": "string", "description": "New file content"},
                "create_parent_dirs": {"type": "boolean", "description": "Create missing parent directories"}
            },
            "required": ["path", "content", "create_parent_dirs"]
        }),
    )
}

pub fn replace() -> Tool {
    tool_template(
        "fs_replace",
        "Replace one exact text occurrence in a UTF-8 text file.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "old": {"type": "string", "description": "Exact text to replace"},
                "new": {"type": "string", "description": "Replacement text"}
            },
            "required": ["path", "old", "new"]
        }),
    )
}

pub fn mkdir() -> Tool {
    tool_template(
        "fs_mkdir",
        "Create a directory and any missing parent directories.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory path"}
            },
            "required": ["path"]
        }),
    )
}

pub fn remove() -> Tool {
    tool_template(
        "fs_remove",
        "Remove a file or an empty directory. Recursive removal requires recursive=true.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to remove"},
                "recursive": {"type": "boolean", "description": "Recursively remove a directory"}
            },
            "required": ["path", "recursive"]
        }),
    )
}

pub fn rename() -> Tool {
    tool_template(
        "fs_rename",
        "Rename or move a file or directory.",
        json!({
            "type": "object",
            "properties": {
                "from": {"type": "string", "description": "Source path"},
                "to": {"type": "string", "description": "Destination path"}
            },
            "required": ["from", "to"]
        }),
    )
}

pub fn copy() -> Tool {
    tool_template(
        "fs_copy",
        "Copy a file.",
        json!({
            "type": "object",
            "properties": {
                "from": {"type": "string", "description": "Source file path"},
                "to": {"type": "string", "description": "Destination file path"}
            },
            "required": ["from", "to"]
        }),
    )
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

pub fn execute_list(run_root: &Path, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = resolve_tool_path(run_root, path);
    match fs::read_dir(&path) {
        Ok(entries) => {
            let entries = entries
                .filter_map(Result::ok)
                .map(|entry| {
                    let entry_path = entry.path();
                    json!({
                        "name": entry.file_name().to_string_lossy(),
                        "path": entry_path.display().to_string(),
                        "kind": path_kind(&entry_path),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "status": "ok",
                "path": path.display().to_string(),
                "entries": entries,
            })
            .to_string()
        }
        Err(error) => json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": error.to_string(),
        })
        .to_string(),
    }
}

pub fn execute_stat(run_root: &Path, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = resolve_tool_path(run_root, path);
    match fs::metadata(&path) {
        Ok(metadata) => json!({
            "status": "ok",
            "path": path.display().to_string(),
            "kind": path_kind(&path),
            "len": metadata.len(),
            "readonly": metadata.permissions().readonly(),
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

pub fn execute_write(run_root: &Path, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let create_parent_dirs = args
        .get("create_parent_dirs")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let path = resolve_tool_path(run_root, path);
    if create_parent_dirs
        && let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": error.to_string(),
        })
        .to_string();
    }
    match fs::write(&path, content) {
        Ok(()) => json!({
            "status": "ok",
            "path": path.display().to_string(),
            "bytes": content.len(),
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

pub fn execute_replace(run_root: &Path, args: &Value) -> String {
    let path_arg = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let old = args.get("old").and_then(Value::as_str).unwrap_or("");
    let new = args.get("new").and_then(Value::as_str).unwrap_or("");
    let path = resolve_tool_path(run_root, path_arg);
    if old.is_empty() {
        return json!({
            "status": "error",
            "path": path.display().to_string(),
            "error": "old must not be empty",
        })
        .to_string();
    }
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            return json!({
                "status": "error",
                "path": path.display().to_string(),
                "error": error.to_string(),
            })
            .to_string();
        }
    };
    let count = content.matches(old).count();
    if count != 1 {
        return json!({
            "status": "error",
            "path": path.display().to_string(),
            "matches": count,
            "error": "old text must match exactly once",
        })
        .to_string();
    }
    let updated = content.replacen(old, new, 1);
    match fs::write(&path, updated) {
        Ok(()) => json!({
            "status": "ok",
            "path": path.display().to_string(),
            "replacements": 1,
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

pub fn execute_mkdir(run_root: &Path, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let path = resolve_tool_path(run_root, path);
    match fs::create_dir_all(&path) {
        Ok(()) => json!({
            "status": "ok",
            "path": path.display().to_string(),
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

pub fn execute_remove(run_root: &Path, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let recursive = args
        .get("recursive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let path = resolve_tool_path(run_root, path);
    let result = if path.is_dir() {
        if recursive {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_dir(&path)
        }
    } else {
        fs::remove_file(&path)
    };
    match result {
        Ok(()) => json!({
            "status": "ok",
            "path": path.display().to_string(),
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

pub fn execute_rename(run_root: &Path, args: &Value) -> String {
    let from = args.get("from").and_then(Value::as_str).unwrap_or(".");
    let to = args.get("to").and_then(Value::as_str).unwrap_or(".");
    let from = resolve_tool_path(run_root, from);
    let to = resolve_tool_path(run_root, to);
    match fs::rename(&from, &to) {
        Ok(()) => json!({
            "status": "ok",
            "from": from.display().to_string(),
            "to": to.display().to_string(),
        })
        .to_string(),
        Err(error) => json!({
            "status": "error",
            "from": from.display().to_string(),
            "to": to.display().to_string(),
            "error": error.to_string(),
        })
        .to_string(),
    }
}

pub fn execute_copy(run_root: &Path, args: &Value) -> String {
    let from = args.get("from").and_then(Value::as_str).unwrap_or(".");
    let to = args.get("to").and_then(Value::as_str).unwrap_or(".");
    let from = resolve_tool_path(run_root, from);
    let to = resolve_tool_path(run_root, to);
    match fs::copy(&from, &to) {
        Ok(bytes) => json!({
            "status": "ok",
            "from": from.display().to_string(),
            "to": to.display().to_string(),
            "bytes": bytes,
        })
        .to_string(),
        Err(error) => json!({
            "status": "error",
            "from": from.display().to_string(),
            "to": to.display().to_string(),
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

fn path_kind(path: &Path) -> &'static str {
    if path.is_dir() {
        "dir"
    } else if path.is_file() {
        "file"
    } else if path.is_symlink() {
        "symlink"
    } else {
        "other"
    }
}

pub(crate) fn resolve_tool_path(run_root: &Path, path: &str) -> PathBuf {
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
