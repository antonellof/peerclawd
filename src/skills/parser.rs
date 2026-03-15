//! SKILL.md file parser.
//!
//! Parses skill files with YAML frontmatter and markdown content.

use std::path::Path;

use thiserror::Error;

use super::{LoadedSkill, SkillManifest, SkillSource, SkillTrust, MAX_SKILL_SIZE};

/// Error parsing a skill file.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("File too large: {size} bytes (max {max} bytes)")]
    FileTooLarge { size: u64, max: u64 },

    #[error("Missing YAML frontmatter (expected --- delimiter)")]
    MissingFrontmatter,

    #[error("Invalid YAML frontmatter: {0}")]
    InvalidYaml(String),

    #[error("Invalid skill name: {0}")]
    InvalidName(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Parse a SKILL.md file from path.
pub fn parse_skill(path: &Path, trust: SkillTrust) -> Result<LoadedSkill, ParseError> {
    // Check file size
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_SKILL_SIZE {
        return Err(ParseError::FileTooLarge {
            size: metadata.len(),
            max: MAX_SKILL_SIZE,
        });
    }

    let content = std::fs::read_to_string(path)?;
    let source = match trust {
        SkillTrust::Local => SkillSource::Local(path.to_path_buf()),
        SkillTrust::Installed => SkillSource::Workspace(path.to_path_buf()),
        SkillTrust::Network => SkillSource::Bundled(path.display().to_string()),
    };

    parse_skill_content(&content, source, trust)
}

/// Parse skill content directly.
pub fn parse_skill_content(
    content: &str,
    source: SkillSource,
    trust: SkillTrust,
) -> Result<LoadedSkill, ParseError> {
    // Split frontmatter and content
    let (manifest, prompt_content) = split_frontmatter(content)?;

    // Validate name
    if !super::validate_skill_name(&manifest.name) {
        return Err(ParseError::InvalidName(manifest.name.clone()));
    }

    // Calculate content hash
    let hash = LoadedSkill::calculate_hash(content);

    // Check requirements
    let requirements_met = check_requirements(&manifest.requires);

    Ok(LoadedSkill {
        manifest,
        prompt_content,
        trust,
        source,
        hash,
        requirements_met,
    })
}

/// Split YAML frontmatter from markdown content.
fn split_frontmatter(content: &str) -> Result<(SkillManifest, String), ParseError> {
    let content = content.trim();

    // Check for frontmatter delimiter
    if !content.starts_with("---") {
        return Err(ParseError::MissingFrontmatter);
    }

    // Find end of frontmatter
    let rest = &content[3..];
    let end_pos = rest.find("\n---")
        .ok_or(ParseError::MissingFrontmatter)?;

    let yaml_content = &rest[..end_pos].trim();
    let markdown_content = &rest[end_pos + 4..].trim();

    // Parse YAML
    let manifest: SkillManifest = serde_yaml::from_str(yaml_content)
        .map_err(|e| ParseError::InvalidYaml(e.to_string()))?;

    Ok((manifest, markdown_content.to_string()))
}

/// Check if skill requirements are met.
fn check_requirements(req: &super::SkillRequirements) -> bool {
    // Check required binaries
    for bin in &req.bins {
        if which::which(bin).is_err() {
            return false;
        }
    }

    // Check required env vars
    for var in &req.env {
        if std::env::var(var).is_err() {
            return false;
        }
    }

    // TODO: Check tools and models availability
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SKILL: &str = r#"---
name: code-review
version: 1.0.0
description: Code review assistant skill
author: PeerClaw
activation:
  keywords:
    - review
    - code review
    - pull request
  tags:
    - development
    - code
requires:
  bins:
    - git
sharing:
  enabled: true
  price: 100
---

# Code Review Assistant

You are a code review assistant. When reviewing code:

1. Check for bugs and security issues
2. Suggest improvements for readability
3. Ensure tests are adequate
4. Follow the project's coding standards

Be constructive and helpful in your feedback.
"#;

    #[test]
    fn test_parse_skill() {
        let result = parse_skill_content(
            SAMPLE_SKILL,
            SkillSource::Bundled("test".to_string()),
            SkillTrust::Local,
        );

        assert!(result.is_ok());
        let skill = result.unwrap();
        assert_eq!(skill.manifest.name, "code-review");
        assert_eq!(skill.manifest.version, "1.0.0");
        assert!(skill.prompt_content.contains("code review assistant"));
        assert!(skill.manifest.activation.keywords.contains(&"review".to_string()));
    }

    #[test]
    fn test_missing_frontmatter() {
        let content = "# Just markdown\n\nNo frontmatter here.";
        let result = parse_skill_content(
            content,
            SkillSource::Bundled("test".to_string()),
            SkillTrust::Local,
        );
        assert!(matches!(result, Err(ParseError::MissingFrontmatter)));
    }
}
