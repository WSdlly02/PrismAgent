use genai::chat::Tool;
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
