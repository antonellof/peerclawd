//! File operations helper for workspace.
//!
//! Provides typed file operations for common workspace file types.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Result;

/// Memory entry in MEMORY.md
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Category (user_preferences, facts, lessons)
    pub category: String,
    /// Entry content
    pub content: String,
    /// Timestamp
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Tags
    pub tags: Vec<String>,
}

/// Parse MEMORY.md content into structured entries
pub fn parse_memory(content: &str) -> Result<Vec<MemoryEntry>> {
    let mut entries = Vec::new();
    let mut current_category = String::new();

    for line in content.lines() {
        let line = line.trim();

        // Section header
        if let Some(stripped) = line.strip_prefix("## ") {
            current_category = stripped.trim().to_lowercase().replace(' ', "_");
            continue;
        }

        // List item
        if line.starts_with("- ") || line.starts_with("* ") {
            let content = line[2..].trim();
            if !content.is_empty() && content != "(Add" && !content.starts_with("(Add") {
                entries.push(MemoryEntry {
                    category: current_category.clone(),
                    content: content.to_string(),
                    timestamp: None,
                    tags: Vec::new(),
                });
            }
        }
    }

    Ok(entries)
}

/// Format memory entries as markdown
pub fn format_memory(entries: &[MemoryEntry]) -> String {
    let mut output = String::from("# Memory\n\n");

    // Group by category
    let mut categories: std::collections::HashMap<&str, Vec<&MemoryEntry>> =
        std::collections::HashMap::new();

    for entry in entries {
        categories
            .entry(&entry.category)
            .or_default()
            .push(entry);
    }

    // Standard order
    let order = ["user_preferences", "important_facts", "lessons_learned"];

    for cat in order {
        if let Some(items) = categories.remove(cat) {
            let title = cat.replace('_', " ");
            let title = title
                .split_whitespace()
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            output.push_str(&format!("## {}\n\n", title));
            for entry in items {
                output.push_str(&format!("- {}\n", entry.content));
            }
            output.push('\n');
        }
    }

    // Remaining categories
    for (cat, items) in categories {
        let title = cat.replace('_', " ");
        output.push_str(&format!("## {}\n\n", title));
        for entry in items {
            output.push_str(&format!("- {}\n", entry.content));
        }
        output.push('\n');
    }

    output
}

/// Heartbeat task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatTask {
    /// Task description
    pub description: String,
    /// Is completed
    pub completed: bool,
    /// Frequency (daily, weekly, monthly)
    pub frequency: Option<String>,
}

/// Parse HEARTBEAT.md
pub fn parse_heartbeat(content: &str) -> Result<Vec<HeartbeatTask>> {
    let mut tasks = Vec::new();
    let mut current_frequency: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();

        // Section with frequency hint
        if line.contains("(daily)") {
            current_frequency = Some("daily".to_string());
        } else if line.contains("(weekly)") {
            current_frequency = Some("weekly".to_string());
        } else if line.contains("(monthly)") {
            current_frequency = Some("monthly".to_string());
        }

        // Checkbox item
        if line.starts_with("- [ ]") || line.starts_with("- [x]") || line.starts_with("- [X]") {
            let completed = line.starts_with("- [x]") || line.starts_with("- [X]");
            let description = line[5..].trim().to_string();

            // Extract frequency from description
            let frequency = if description.contains("(daily)") {
                Some("daily".to_string())
            } else if description.contains("(weekly)") {
                Some("weekly".to_string())
            } else if description.contains("(monthly)") {
                Some("monthly".to_string())
            } else {
                current_frequency.clone()
            };

            tasks.push(HeartbeatTask {
                description: description
                    .replace("(daily)", "")
                    .replace("(weekly)", "")
                    .replace("(monthly)", "")
                    .trim()
                    .to_string(),
                completed,
                frequency,
            });
        }
    }

    Ok(tasks)
}

/// Daily log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyLogEntry {
    /// Timestamp
    pub time: String,
    /// Entry content
    pub content: String,
}

/// Parse daily log file
pub fn parse_daily_log(content: &str) -> Result<Vec<DailyLogEntry>> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Format: - [HH:MM:SS] content
        if line.starts_with("- [") {
            if let Some(end_bracket) = line.find(']') {
                let time = line[3..end_bracket].to_string();
                let content = line[end_bracket + 1..].trim().to_string();
                entries.push(DailyLogEntry { time, content });
            }
        }
    }

    Ok(entries)
}

/// Get file extension for a path
pub fn get_extension(path: &str) -> Option<&str> {
    Path::new(path).extension().and_then(|e| e.to_str())
}

/// Check if path is a markdown file
pub fn is_markdown(path: &str) -> bool {
    matches!(get_extension(path), Some("md") | Some("markdown"))
}

/// Check if path is in a protected directory
pub fn is_protected_path(path: &str) -> bool {
    let protected = ["context/secrets", ".git", "node_modules"];
    protected.iter().any(|p| path.contains(p))
}

/// Sanitize filename
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory() {
        let content = r#"# Memory

## User Preferences

- Prefers dark mode
- Uses vim keybindings

## Important Facts

- Project uses Rust
"#;

        let entries = parse_memory(content).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].category, "user_preferences");
        assert_eq!(entries[0].content, "Prefers dark mode");
    }

    #[test]
    fn test_parse_heartbeat() {
        let content = r#"# Heartbeat

## Regular Checks

- [ ] Check pending tasks
- [x] Review logs

## Daily Tasks (daily)

- [ ] Update memory
"#;

        let tasks = parse_heartbeat(content).unwrap();
        assert_eq!(tasks.len(), 3);
        assert!(!tasks[0].completed);
        assert!(tasks[1].completed);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("hello world.txt"), "hello_world.txt");
        assert_eq!(sanitize_filename("file/with:bad*chars"), "file_with_bad_chars");
    }
}
