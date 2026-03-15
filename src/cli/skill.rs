//! `peerclaw skill` commands - Skill management.

use clap::Subcommand;
use std::sync::Arc;

use crate::skills::SkillRegistry;

#[derive(Subcommand)]
pub enum SkillCommand {
    /// List all available skills
    List {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,

        /// Show network skills
        #[arg(long)]
        network: bool,
    },

    /// Show skill information
    Info {
        /// Skill name
        name: String,
    },

    /// Scan and reload skills from disk
    Scan,

    /// Install a skill from content or URL
    Install {
        /// Path to SKILL.md file or URL
        source: String,
    },

    /// Remove a skill
    Remove {
        /// Skill name
        name: String,
    },

    /// Create a new skill template
    Create {
        /// Skill name
        name: String,
    },
}

pub async fn run(cmd: SkillCommand) -> anyhow::Result<()> {
    // Create a skill registry
    let skills_dir = crate::bootstrap::base_dir().join("skills");
    let registry = Arc::new(
        SkillRegistry::new(skills_dir.clone(), "cli-user".to_string())
            .map_err(|e| anyhow::anyhow!("Failed to create skill registry: {}", e))?
    );

    // Scan for skills
    let _ = registry.scan().await;

    match cmd {
        SkillCommand::List { verbose, network } => {
            let skills = if network {
                registry.list_all().await
            } else {
                registry.list_local().await.into_iter().map(|s| crate::skills::SkillInfo {
                    name: s.name().to_string(),
                    version: s.manifest.version.clone(),
                    description: s.description().to_string(),
                    trust: s.trust,
                    available: s.is_available(),
                    provider: "local".to_string(),
                    price: s.manifest.sharing.price,
                }).collect()
            };

            println!("\n{:=<60}", "");
            println!(" Available Skills ({} total)", skills.len());
            println!("{:=<60}\n", "");

            if skills.is_empty() {
                println!("  No skills found.");
                println!();
                println!("  Skills directory: {}", skills_dir.display());
                println!("  Create a SKILL.md file to add a skill.");
            } else {
                for skill in &skills {
                    if verbose {
                        println!("  {} (v{})", skill.name, skill.version);
                        println!("    Description: {}", skill.description);
                        println!("    Trust: {:?}, Available: {}", skill.trust, skill.available);
                        if skill.price > 0 {
                            println!("    Price: {} micro-PCLAW", skill.price);
                        }
                        println!();
                    } else {
                        let status = if skill.available { "✓" } else { "✗" };
                        println!("  {} {:20} {} (v{})",
                            status,
                            skill.name,
                            truncate(&skill.description, 30),
                            skill.version
                        );
                    }
                }
            }
            println!();
        }

        SkillCommand::Info { name } => {
            if let Some(skill) = registry.get(&name).await {
                println!("\n{:=<60}", "");
                println!(" Skill: {}", skill.name());
                println!("{:=<60}\n", "");
                println!("  Version:     {}", skill.manifest.version);
                println!("  Description: {}", skill.description());
                println!("  Trust:       {:?}", skill.trust);
                println!("  Available:   {}", skill.is_available());
                println!("  Hash:        {}", skill.hash);

                if let Some(author) = &skill.manifest.author {
                    println!("  Author:      {}", author);
                }

                // Keywords
                if !skill.manifest.activation.keywords.is_empty() {
                    println!("\n  Keywords: {}", skill.manifest.activation.keywords.join(", "));
                }

                // Tags
                if !skill.manifest.activation.tags.is_empty() {
                    println!("  Tags:     {}", skill.manifest.activation.tags.join(", "));
                }

                // Requirements
                if !skill.manifest.requires.bins.is_empty() {
                    println!("\n  Required binaries: {}", skill.manifest.requires.bins.join(", "));
                }
                if !skill.manifest.requires.env.is_empty() {
                    println!("  Required env vars: {}", skill.manifest.requires.env.join(", "));
                }

                // Sharing settings
                if skill.manifest.sharing.enabled {
                    println!("\n  Sharing: Enabled");
                    println!("    Price: {} micro-PCLAW per use", skill.manifest.sharing.price);
                    if let Some(limit) = skill.manifest.sharing.rate_limit {
                        println!("    Rate limit: {} uses/day", limit);
                    }
                }

                // Prompt preview
                println!("\n  Prompt Preview:");
                println!("  {:-<56}", "");
                let preview = truncate(skill.prompt(), 500);
                for line in preview.lines().take(10) {
                    println!("  {}", line);
                }
                if skill.prompt().len() > 500 {
                    println!("  ... (truncated)");
                }
            } else {
                println!("Skill '{}' not found", name);
                println!("Use 'peerclaw skill list' to see available skills.");
            }
        }

        SkillCommand::Scan => {
            println!("Scanning skills directory: {}", skills_dir.display());

            match registry.scan().await {
                Ok(count) => {
                    println!("Loaded {} skills", count);
                }
                Err(e) => {
                    println!("Error scanning skills: {}", e);
                }
            }
        }

        SkillCommand::Install { source } => {
            // Check if it's a file path
            let path = std::path::Path::new(&source);
            if path.exists() {
                let content = std::fs::read_to_string(path)?;
                match registry.install(&content, crate::skills::SkillTrust::Installed).await {
                    Ok(skill) => {
                        println!("Installed skill: {} (v{})", skill.name(), skill.manifest.version);
                    }
                    Err(e) => {
                        println!("Failed to install skill: {}", e);
                    }
                }
            } else {
                println!("Source '{}' not found.", source);
                println!("Provide a path to a SKILL.md file.");
            }
        }

        SkillCommand::Remove { name } => {
            if registry.remove(&name).await {
                println!("Removed skill: {}", name);
            } else {
                println!("Skill '{}' not found", name);
            }
        }

        SkillCommand::Create { name } => {
            let skill_path = skills_dir.join(format!("{}.md", &name));
            if skill_path.exists() {
                println!("Skill '{}' already exists at {}", name, skill_path.display());
                return Ok(());
            }

            let template = format!(r#"---
name: {}
version: 1.0.0
description: A custom skill
author: Your Name
activation:
  keywords:
    - {}
  tags:
    - custom
requires:
  bins: []
  env: []
sharing:
  enabled: false
  price: 0
---

# {}

You are a helpful assistant with expertise in [your domain].

When helping users:
1. Be clear and concise
2. Provide examples when helpful
3. Ask clarifying questions if needed

## Guidelines

- Follow best practices
- Be security-conscious
- Cite sources when applicable
"#, name, name, name.replace("-", " ").to_uppercase());

            std::fs::write(&skill_path, template)?;
            println!("Created skill template: {}", skill_path.display());
            println!();
            println!("Edit the file to customize your skill, then run:");
            println!("  peerclaw skill scan");
        }
    }

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
