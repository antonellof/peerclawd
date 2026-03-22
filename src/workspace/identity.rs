//! Identity management for workspace.
//!
//! Parses IDENTITY.md to extract agent personality and configuration.

use serde::{Deserialize, Serialize};

use super::Result;

/// Identity configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdentityConfig {
    /// Agent name
    pub name: Option<String>,
    /// Agent nature/role
    pub nature: Option<String>,
    /// Communication vibe
    pub vibe: Option<String>,
    /// Custom system prompt additions
    pub system_prompt: Option<String>,
}

/// Parsed agent identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// Agent name
    pub name: String,
    /// Agent nature/role description
    pub nature: String,
    /// Communication style/vibe
    pub vibe: String,
    /// Personality traits
    pub traits: Vec<String>,
    /// Communication style guidelines
    pub communication_style: Vec<String>,
    /// Core values
    pub values: Vec<String>,
    /// Custom sections
    pub custom_sections: std::collections::HashMap<String, Vec<String>>,
}

impl Default for Identity {
    fn default() -> Self {
        Self {
            name: "PeerClaw".to_string(),
            nature: "Helpful AI assistant".to_string(),
            vibe: "Professional yet friendly".to_string(),
            traits: vec![
                "Helpful and knowledgeable".to_string(),
                "Concise but thorough".to_string(),
                "Security-conscious".to_string(),
            ],
            communication_style: vec![
                "Clear and direct".to_string(),
                "Uses examples when helpful".to_string(),
                "Admits uncertainty honestly".to_string(),
            ],
            values: Vec::new(),
            custom_sections: std::collections::HashMap::new(),
        }
    }
}

impl Identity {
    /// Parse identity from IDENTITY.md content
    pub fn parse(content: &str) -> Result<Self> {
        // Start with empty collections - don't use defaults when parsing
        let mut identity = Identity {
            name: "PeerClaw".to_string(),
            nature: "Helpful AI assistant".to_string(),
            vibe: "Professional yet friendly".to_string(),
            traits: Vec::new(),
            communication_style: Vec::new(),
            values: Vec::new(),
            custom_sections: std::collections::HashMap::new(),
        };
        let mut current_section = String::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines
            if line.is_empty() {
                continue;
            }

            // Handle YAML-like frontmatter (name: value)
            if line.contains(':') && !line.starts_with('#') && !line.starts_with('-') {
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim().to_lowercase();
                    let value = parts[1].trim().to_string();

                    if !value.is_empty() {
                        match key.as_str() {
                            "name" => identity.name = value,
                            "nature" => identity.nature = value,
                            "vibe" | "style" => identity.vibe = value,
                            _ => {}
                        }
                    }
                }
                continue;
            }

            // Section header
            if let Some(stripped) = line.strip_prefix("## ") {
                current_section = stripped.trim().to_lowercase().replace(' ', "_");
                continue;
            }

            // H1 header (title)
            if line.starts_with("# ") {
                continue;
            }

            // List items
            if line.starts_with("- ") || line.starts_with("* ") {
                let item = line[2..].trim().to_string();
                if item.is_empty() {
                    continue;
                }

                match current_section.as_str() {
                    "traits" | "personality" | "characteristics" => {
                        identity.traits.push(item);
                    }
                    "communication_style" | "style" | "communication" => {
                        identity.communication_style.push(item);
                    }
                    "values" | "core_values" | "principles" => {
                        identity.values.push(item);
                    }
                    section if !section.is_empty() => {
                        identity
                            .custom_sections
                            .entry(section.to_string())
                            .or_default()
                            .push(item);
                    }
                    _ => {}
                }
            }
        }

        Ok(identity)
    }

    /// Generate system prompt from identity
    pub fn to_system_prompt(&self) -> String {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "You are {}, a {}.\n\n",
            self.name, self.nature
        ));

        prompt.push_str(&format!("Communication style: {}\n\n", self.vibe));

        if !self.traits.is_empty() {
            prompt.push_str("Personality traits:\n");
            for trait_item in &self.traits {
                prompt.push_str(&format!("- {}\n", trait_item));
            }
            prompt.push('\n');
        }

        if !self.communication_style.is_empty() {
            prompt.push_str("Communication guidelines:\n");
            for guideline in &self.communication_style {
                prompt.push_str(&format!("- {}\n", guideline));
            }
            prompt.push('\n');
        }

        if !self.values.is_empty() {
            prompt.push_str("Core values:\n");
            for value in &self.values {
                prompt.push_str(&format!("- {}\n", value));
            }
            prompt.push('\n');
        }

        prompt
    }

    /// Format as markdown
    pub fn to_markdown(&self) -> String {
        let mut output = String::from("# Identity\n\n");

        output.push_str(&format!("name: {}\n", self.name));
        output.push_str(&format!("nature: {}\n", self.nature));
        output.push_str(&format!("vibe: {}\n\n", self.vibe));

        if !self.traits.is_empty() {
            output.push_str("## Traits\n\n");
            for trait_item in &self.traits {
                output.push_str(&format!("- {}\n", trait_item));
            }
            output.push('\n');
        }

        if !self.communication_style.is_empty() {
            output.push_str("## Communication Style\n\n");
            for guideline in &self.communication_style {
                output.push_str(&format!("- {}\n", guideline));
            }
            output.push('\n');
        }

        if !self.values.is_empty() {
            output.push_str("## Values\n\n");
            for value in &self.values {
                output.push_str(&format!("- {}\n", value));
            }
            output.push('\n');
        }

        for (section, items) in &self.custom_sections {
            let title = section.replace('_', " ");
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
            for item in items {
                output.push_str(&format!("- {}\n", item));
            }
            output.push('\n');
        }

        output
    }

    /// Get a brief description
    pub fn brief(&self) -> String {
        format!("{} - {}", self.name, self.nature)
    }
}

/// Soul configuration (from SOUL.md)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Soul {
    /// Core purpose
    pub purpose: String,
    /// Fundamental principles
    pub principles: Vec<String>,
    /// Ethical boundaries
    pub boundaries: Vec<String>,
    /// Goals
    pub goals: Vec<String>,
}

impl Soul {
    /// Parse from SOUL.md content
    pub fn parse(content: &str) -> Result<Self> {
        let mut soul = Soul::default();
        let mut current_section = String::new();

        for line in content.lines() {
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            // Section header
            if let Some(stripped) = line.strip_prefix("## ") {
                current_section = stripped.trim().to_lowercase().replace(' ', "_");
                continue;
            }

            // Purpose line
            if line.starts_with("purpose:") || line.starts_with("Purpose:") {
                soul.purpose = line.split_once(':').map(|x| x.1).unwrap_or("").trim().to_string();
                continue;
            }

            // List items
            if line.starts_with("- ") || line.starts_with("* ") {
                let item = line[2..].trim().to_string();
                if item.is_empty() {
                    continue;
                }

                match current_section.as_str() {
                    "principles" | "core_principles" => {
                        soul.principles.push(item);
                    }
                    "boundaries" | "ethical_boundaries" | "limits" => {
                        soul.boundaries.push(item);
                    }
                    "goals" | "objectives" => {
                        soul.goals.push(item);
                    }
                    _ => {}
                }
            }
        }

        Ok(soul)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_identity() {
        let content = r#"# Identity

name: TestBot
nature: Test assistant
vibe: Casual and helpful

## Traits

- Friendly
- Efficient
- Accurate

## Communication Style

- Uses simple language
- Provides examples
"#;

        let identity = Identity::parse(content).unwrap();
        assert_eq!(identity.name, "TestBot");
        assert_eq!(identity.nature, "Test assistant");
        assert_eq!(identity.vibe, "Casual and helpful");
        assert_eq!(identity.traits.len(), 3);
        assert_eq!(identity.communication_style.len(), 2);
    }

    #[test]
    fn test_identity_to_system_prompt() {
        let identity = Identity {
            name: "Helper".to_string(),
            nature: "AI assistant".to_string(),
            vibe: "Professional".to_string(),
            traits: vec!["Helpful".to_string()],
            communication_style: vec!["Clear".to_string()],
            values: Vec::new(),
            custom_sections: std::collections::HashMap::new(),
        };

        let prompt = identity.to_system_prompt();
        assert!(prompt.contains("You are Helper"));
        assert!(prompt.contains("AI assistant"));
    }

    #[test]
    fn test_parse_soul() {
        let content = r#"# Soul

purpose: Help users accomplish their goals

## Principles

- Always be honest
- Respect privacy

## Boundaries

- Never generate harmful content
- Never leak credentials
"#;

        let soul = Soul::parse(content).unwrap();
        assert!(soul.purpose.contains("Help users"));
        assert_eq!(soul.principles.len(), 2);
        assert_eq!(soul.boundaries.len(), 2);
    }
}
