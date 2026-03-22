//! Shell execution tool for running commands.
//!
//! Provides controlled command execution with:
//! - Timeout enforcement
//! - Output capture and truncation
//! - Blocked command patterns for safety
//! - Environment scrubbing

use std::collections::HashSet;
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::tools::tool::{
    Tool, ToolContext, ToolError, ToolOutput, ToolDomain, ApprovalRequirement,
    require_str, optional_str, optional_i64,
};

/// Maximum output size before truncation (64KB).
const MAX_OUTPUT_SIZE: usize = 64 * 1024;

/// Default command timeout.
#[allow(dead_code)]
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Commands that are always blocked for safety.
static BLOCKED_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "rm -rf /",
        "rm -rf /*",
        ":(){ :|:& };:", // Fork bomb
        "dd if=/dev/zero",
        "mkfs",
        "chmod -R 777 /",
        "> /dev/sda",
        "curl | sh",
        "wget | sh",
        "curl | bash",
        "wget | bash",
    ])
});

/// Patterns that indicate potentially dangerous commands.
static DANGEROUS_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "sudo ",
        "doas ",
        " | sh",
        " | bash",
        " | zsh",
        "eval ",
        "$(curl",
        "$(wget",
        "/etc/passwd",
        "/etc/shadow",
        "~/.ssh",
        ".bash_history",
        "id_rsa",
    ]
});

/// Patterns that should NEVER be auto-approved.
#[allow(dead_code)]
static NEVER_AUTO_APPROVE_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "rm -rf",
        "rm -fr",
        "chmod -r 777",
        "chmod 777",
        "chown -r",
        "shutdown",
        "reboot",
        "poweroff",
        "iptables",
        "useradd",
        "userdel",
        "passwd",
        "visudo",
        "crontab",
        "kill -9",
        "killall",
        "pkill",
        "docker rm",
        "docker rmi",
        "docker system prune",
        "git push --force",
        "git push -f",
        "git reset --hard",
        "DROP TABLE",
        "DROP DATABASE",
        "TRUNCATE",
    ]
});

/// Environment variables safe to forward to child processes.
static SAFE_ENV_VARS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "PATH",
        "HOME",
        "USER",
        "SHELL",
        "TERM",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TZ",
        "TMPDIR",
        "TEMP",
        "TMP",
        // Build tools
        "CC",
        "CXX",
        "CFLAGS",
        "LDFLAGS",
        // Rust
        "CARGO_HOME",
        "RUSTUP_HOME",
        // Node
        "NODE_PATH",
        "NPM_CONFIG_PREFIX",
        // Python
        "PYTHONPATH",
        "VIRTUAL_ENV",
        // Go
        "GOPATH",
        "GOROOT",
    ])
});

/// Shell execution tool.
pub struct ShellTool {
    default_shell: String,
}

impl ShellTool {
    pub fn new() -> Self {
        let default_shell = if cfg!(windows) {
            "cmd".to_string()
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        };

        Self { default_shell }
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute shell commands. Returns stdout, stderr, and exit code. \
         Use for system commands, build tools, git operations, etc. \
         Commands run in a scrubbed environment (no API keys exposed)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (default: current directory)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 120)"
                },
                "env": {
                    "type": "object",
                    "description": "Additional environment variables"
                },
                "stdin": {
                    "type": "string",
                    "description": "Input to pass to stdin"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let command = require_str(&params, "command")?;
        let cwd = optional_str(&params, "cwd");
        let timeout_secs = optional_i64(&params, "timeout", 120) as u64;

        // Security checks
        if is_blocked(command) {
            return Err(ToolError::NotAuthorized(format!(
                "Command blocked for safety: {}",
                command
            )));
        }

        if has_dangerous_pattern(command) {
            tracing::warn!(command = %command, "Executing command with dangerous pattern");
        }

        // Build command
        let working_dir = cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| ctx.working_dir.clone());

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = Command::new(&self.default_shell);
            c.args(["-c", command]);
            c
        };

        cmd.current_dir(&working_dir);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Scrub environment - only pass safe variables
        cmd.env_clear();
        for (key, value) in std::env::vars() {
            if SAFE_ENV_VARS.contains(key.as_str()) {
                cmd.env(&key, &value);
            }
        }

        // Add user-specified env vars
        if let Some(env) = params.get("env").and_then(|v| v.as_object()) {
            for (key, value) in env {
                if let Some(value_str) = value.as_str() {
                    cmd.env(key, value_str);
                }
            }
        }

        // Spawn process
        let mut child = cmd.spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn: {}", e)))?;

        // Write stdin if provided
        if let Some(stdin_data) = params.get("stdin").and_then(|v| v.as_str()) {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(stdin_data.as_bytes()).await;
            }
        }

        // Wait with timeout
        let timeout = Duration::from_secs(timeout_secs);
        let output = tokio::time::timeout(timeout, async {
            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            if let Some(mut stdout) = child.stdout.take() {
                let _ = stdout.read_to_end(&mut stdout_buf).await;
            }
            if let Some(mut stderr) = child.stderr.take() {
                let _ = stderr.read_to_end(&mut stderr_buf).await;
            }

            let status = child.wait().await;
            (stdout_buf, stderr_buf, status)
        }).await;

        match output {
            Ok((stdout_buf, stderr_buf, status)) => {
                let exit_code = status
                    .map(|s| s.code().unwrap_or(-1))
                    .unwrap_or(-1);

                let stdout = truncate_output(&stdout_buf, MAX_OUTPUT_SIZE);
                let stderr = truncate_output(&stderr_buf, MAX_OUTPUT_SIZE);

                let result = serde_json::json!({
                    "command": command,
                    "cwd": working_dir.display().to_string(),
                    "exit_code": exit_code,
                    "stdout": stdout,
                    "stderr": stderr,
                    "stdout_truncated": stdout_buf.len() > MAX_OUTPUT_SIZE,
                    "stderr_truncated": stderr_buf.len() > MAX_OUTPUT_SIZE,
                    "elapsed_ms": start.elapsed().as_millis(),
                });

                if exit_code == 0 {
                    Ok(ToolOutput::success(result, start.elapsed()))
                } else {
                    Ok(ToolOutput::failure(
                        format!("Command exited with code {}", exit_code),
                        start.elapsed(),
                    ).with_warning(format!("Exit code: {}", exit_code)))
                }
            }
            Err(_) => {
                // Timeout - kill the process
                let _ = child.kill().await;
                Err(ToolError::Timeout(timeout_secs))
            }
        }
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local // Shell only runs locally
    }

    fn requires_sanitization(&self) -> bool {
        true // Command output could contain anything
    }

    fn rate_limit(&self) -> Option<u32> {
        Some(30) // 30 commands per minute
    }
}

fn is_blocked(command: &str) -> bool {
    let lower = command.to_lowercase();
    BLOCKED_COMMANDS.iter().any(|blocked| lower.contains(*blocked))
}

fn has_dangerous_pattern(command: &str) -> bool {
    let lower = command.to_lowercase();
    DANGEROUS_PATTERNS.iter().any(|pattern| lower.contains(*pattern))
}

fn truncate_output(bytes: &[u8], max_size: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() > max_size {
        let truncated = &s[..max_size];
        // Find last newline for clean truncation
        if let Some(pos) = truncated.rfind('\n') {
            format!("{}...\n[truncated, {} bytes total]", &truncated[..pos], bytes.len())
        } else {
            format!("{}...\n[truncated, {} bytes total]", truncated, bytes.len())
        }
    } else {
        s.to_string()
    }
}

/// Check if a command requires explicit approval (even if auto-approved).
#[allow(dead_code)]
pub fn requires_explicit_approval(command: &str) -> bool {
    let lower = command.to_lowercase();
    NEVER_AUTO_APPROVE_PATTERNS.iter().any(|pattern| lower.contains(*pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_commands() {
        assert!(is_blocked("rm -rf /"));
        assert!(is_blocked("rm -rf /*"));
        assert!(!is_blocked("ls -la"));
    }

    #[test]
    fn test_dangerous_patterns() {
        assert!(has_dangerous_pattern("sudo apt install"));
        assert!(has_dangerous_pattern("cat /etc/passwd"));
        assert!(!has_dangerous_pattern("cargo build"));
    }

    #[test]
    fn test_requires_explicit_approval() {
        assert!(requires_explicit_approval("rm -rf ./build"));
        assert!(requires_explicit_approval("git push --force"));
        assert!(!requires_explicit_approval("git push"));
    }

    #[tokio::test]
    async fn test_shell_echo() {
        let tool = ShellTool::new();
        let ctx = ToolContext::local("test".to_string());

        let result = tool.execute(
            serde_json::json!({"command": "echo hello"}),
            &ctx,
        ).await.unwrap();

        assert!(result.success);
        assert!(result.data["stdout"].as_str().unwrap().contains("hello"));
    }
}
