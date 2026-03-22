//! File system tools: read, write, list.

use std::path::{Path, PathBuf};
use std::time::Instant;

use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncReadExt;

use crate::tools::tool::{
    Tool, ToolContext, ToolError, ToolOutput, ToolDomain, ApprovalRequirement,
    require_str, optional_str, optional_i64, optional_bool,
};

/// Maximum file size to read (10 MB).
const MAX_READ_SIZE: u64 = 10 * 1024 * 1024;

/// Protected paths that should never be accessed.
const PROTECTED_PATHS: &[&str] = &[
    "/etc/passwd",
    "/etc/shadow",
    "/etc/sudoers",
    ".ssh/id_rsa",
    ".ssh/id_ed25519",
    ".env",
    ".bash_history",
    ".zsh_history",
];

/// File read tool.
pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read contents of a file. Supports text and binary files. \
         For binary files, returns base64-encoded content."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "encoding": {
                    "type": "string",
                    "description": "Encoding: utf8 (default) or base64 for binary",
                    "enum": ["utf8", "base64"]
                },
                "offset": {
                    "type": "integer",
                    "description": "Start offset in bytes (for partial reads)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum bytes to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let path_str = require_str(&params, "path")?;
        let encoding = optional_str(&params, "encoding").unwrap_or("utf8");
        let offset = optional_i64(&params, "offset", 0) as u64;
        let limit = optional_i64(&params, "limit", MAX_READ_SIZE as i64) as u64;

        // Security check
        if is_protected_path(path_str) {
            return Err(ToolError::NotAuthorized(format!("Access to {} is not allowed", path_str)));
        }

        let path = resolve_path(path_str, &ctx.working_dir);

        // Check file exists and get metadata
        let metadata = fs::metadata(&path).await
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot access file: {}", e)))?;

        if !metadata.is_file() {
            return Err(ToolError::ExecutionFailed("Path is not a file".to_string()));
        }

        let file_size = metadata.len();
        if file_size > MAX_READ_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "File too large: {} bytes (max {} bytes). Use offset/limit for partial reads.",
                file_size, MAX_READ_SIZE
            )));
        }

        // Read file
        let mut file = fs::File::open(&path).await
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot open file: {}", e)))?;

        // Seek if offset specified
        if offset > 0 {
            use tokio::io::AsyncSeekExt;
            file.seek(std::io::SeekFrom::Start(offset)).await
                .map_err(|e| ToolError::ExecutionFailed(format!("Cannot seek: {}", e)))?;
        }

        let mut buffer = vec![0u8; limit.min(file_size - offset) as usize];
        let bytes_read = file.read(&mut buffer).await
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot read file: {}", e)))?;
        buffer.truncate(bytes_read);

        let result = if encoding == "base64" {
            use base64::Engine;
            serde_json::json!({
                "path": path.display().to_string(),
                "size": file_size,
                "bytes_read": bytes_read,
                "encoding": "base64",
                "content": base64::engine::general_purpose::STANDARD.encode(&buffer),
            })
        } else {
            let content = String::from_utf8(buffer)
                .map_err(|_| ToolError::ExecutionFailed(
                    "File is not valid UTF-8. Use encoding: base64 for binary files.".to_string()
                ))?;

            serde_json::json!({
                "path": path.display().to_string(),
                "size": file_size,
                "bytes_read": bytes_read,
                "encoding": "utf8",
                "content": content,
            })
        };

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        true // File content could be malicious
    }
}

/// File write tool.
pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist. \
         Can append to existing files or overwrite them."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write (text or base64 for binary)"
                },
                "encoding": {
                    "type": "string",
                    "description": "Content encoding: utf8 (default) or base64",
                    "enum": ["utf8", "base64"]
                },
                "append": {
                    "type": "boolean",
                    "description": "Append to file instead of overwriting (default: false)"
                },
                "create_dirs": {
                    "type": "boolean",
                    "description": "Create parent directories if needed (default: true)"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let path_str = require_str(&params, "path")?;
        let content = require_str(&params, "content")?;
        let encoding = optional_str(&params, "encoding").unwrap_or("utf8");
        let append = optional_bool(&params, "append", false);
        let create_dirs = optional_bool(&params, "create_dirs", true);

        // Security check
        if is_protected_path(path_str) {
            return Err(ToolError::NotAuthorized(format!("Access to {} is not allowed", path_str)));
        }

        let path = resolve_path(path_str, &ctx.working_dir);

        // Create parent directories if needed
        if create_dirs {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await
                    .map_err(|e| ToolError::ExecutionFailed(format!("Cannot create directory: {}", e)))?;
            }
        }

        // Decode content if base64
        let bytes = if encoding == "base64" {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.decode(content)
                .map_err(|e| ToolError::InvalidParameters(format!("Invalid base64: {}", e)))?
        } else {
            content.as_bytes().to_vec()
        };

        // Write file
        if append {
            use tokio::io::AsyncWriteExt;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Cannot open file: {}", e)))?;

            file.write_all(&bytes).await
                .map_err(|e| ToolError::ExecutionFailed(format!("Cannot write: {}", e)))?;
        } else {
            fs::write(&path, &bytes).await
                .map_err(|e| ToolError::ExecutionFailed(format!("Cannot write file: {}", e)))?;
        }

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "bytes_written": bytes.len(),
            "mode": if append { "append" } else { "overwrite" },
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// File list tool.
pub struct FileListTool;

#[async_trait]
impl Tool for FileListTool {
    fn name(&self) -> &str {
        "file_list"
    }

    fn description(&self) -> &str {
        "List files and directories in a path. Returns file names, sizes, and types."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (default: current directory)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter results (e.g., *.rs)"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "List recursively (default: false)"
                },
                "include_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files (default: false)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of entries to return"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let path_str = optional_str(&params, "path").unwrap_or(".");
        let recursive = optional_bool(&params, "recursive", false);
        let include_hidden = optional_bool(&params, "include_hidden", false);
        let limit = optional_i64(&params, "limit", 1000) as usize;

        let path = resolve_path(path_str, &ctx.working_dir);

        let entries = if recursive {
            list_recursive(&path, include_hidden, limit).await?
        } else {
            list_directory(&path, include_hidden, limit).await?
        };

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "count": entries.len(),
            "entries": entries,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

async fn list_directory(path: &PathBuf, include_hidden: bool, limit: usize) -> Result<Vec<serde_json::Value>, ToolError> {
    let mut entries = Vec::new();
    let mut read_dir = fs::read_dir(path).await
        .map_err(|e| ToolError::ExecutionFailed(format!("Cannot read directory: {}", e)))?;

    while let Some(entry) = read_dir.next_entry().await
        .map_err(|e| ToolError::ExecutionFailed(format!("Error reading entry: {}", e)))?
    {
        if entries.len() >= limit {
            break;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files unless requested
        if !include_hidden && name.starts_with('.') {
            continue;
        }

        let metadata = entry.metadata().await.ok();
        let file_type = if metadata.as_ref().is_some_and(|m| m.is_dir()) {
            "directory"
        } else if metadata.as_ref().is_some_and(|m| m.is_symlink()) {
            "symlink"
        } else {
            "file"
        };

        entries.push(serde_json::json!({
            "name": name,
            "type": file_type,
            "size": metadata.as_ref().map(|m| m.len()).unwrap_or(0),
            "path": entry.path().display().to_string(),
        }));
    }

    entries.sort_by(|a, b| {
        let a_type = a["type"].as_str().unwrap_or("");
        let b_type = b["type"].as_str().unwrap_or("");
        let a_name = a["name"].as_str().unwrap_or("");
        let b_name = b["name"].as_str().unwrap_or("");
        (a_type, a_name).cmp(&(b_type, b_name))
    });

    Ok(entries)
}

async fn list_recursive(path: &Path, include_hidden: bool, limit: usize) -> Result<Vec<serde_json::Value>, ToolError> {
    let mut entries = Vec::new();
    let mut stack = vec![path.to_path_buf()];

    while let Some(current) = stack.pop() {
        if entries.len() >= limit {
            break;
        }

        let dir_entries = list_directory(&current, include_hidden, limit - entries.len()).await?;
        for entry in dir_entries {
            if entry["type"] == "directory" {
                if let Some(entry_path) = entry["path"].as_str() {
                    stack.push(PathBuf::from(entry_path));
                }
            }
            entries.push(entry);
            if entries.len() >= limit {
                break;
            }
        }
    }

    Ok(entries)
}

fn resolve_path(path: &str, working_dir: &std::path::Path) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else if path.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            home.join(path.strip_prefix("~").unwrap_or(&path))
        } else {
            path
        }
    } else {
        working_dir.join(path)
    }
}

fn is_protected_path(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    PROTECTED_PATHS.iter().any(|p| path_lower.contains(*p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_file_write_and_read() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        let ctx = ToolContext::local("test".to_string());

        // Write
        let write_result = FileWriteTool.execute(
            serde_json::json!({
                "path": file_path.to_str().unwrap(),
                "content": "Hello, World!"
            }),
            &ctx,
        ).await.unwrap();
        assert!(write_result.success);

        // Read
        let read_result = FileReadTool.execute(
            serde_json::json!({
                "path": file_path.to_str().unwrap()
            }),
            &ctx,
        ).await.unwrap();
        assert!(read_result.success);
        assert_eq!(read_result.data["content"], "Hello, World!");
    }

    #[tokio::test]
    async fn test_file_list() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "a").await.unwrap();
        fs::write(dir.path().join("b.txt"), "b").await.unwrap();

        let ctx = ToolContext::local("test".to_string());
        let result = FileListTool.execute(
            serde_json::json!({
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        ).await.unwrap();

        assert!(result.success);
        assert_eq!(result.data["count"], 2);
    }
}
