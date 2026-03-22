//! Skill registry for managing and discovering skills.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::{LoadedSkill, SkillAnnouncement, SkillSource, SkillTrust};
use super::parser::{parse_skill, ParseError};

/// Skill registry manages local and network skills.
pub struct SkillRegistry {
    /// Local peer ID
    local_peer_id: String,
    /// Skills directory
    skills_dir: PathBuf,
    /// Loaded local skills
    local_skills: RwLock<HashMap<String, Arc<LoadedSkill>>>,
    /// Network skills (from other peers)
    network_skills: RwLock<HashMap<String, Vec<SkillAnnouncement>>>,
}

impl SkillRegistry {
    /// Create a new skill registry.
    pub fn new(skills_dir: PathBuf, local_peer_id: String) -> std::io::Result<Self> {
        std::fs::create_dir_all(&skills_dir)?;

        Ok(Self {
            local_peer_id,
            skills_dir,
            local_skills: RwLock::new(HashMap::new()),
            network_skills: RwLock::new(HashMap::new()),
        })
    }

    /// Scan skills directory and load all skills.
    pub async fn scan(&self) -> Result<usize, ScanError> {
        let mut count = 0;
        let mut skills = self.local_skills.write().await;
        skills.clear();

        // Scan skills directory
        let entries = std::fs::read_dir(&self.skills_dir)
            .map_err(|e| ScanError::IoError(e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();

            // Check for SKILL.md files or directories with SKILL.md
            let skill_file = if path.is_dir() {
                path.join("SKILL.md")
            } else if path.extension().is_some_and(|e| e == "md") {
                path.clone()
            } else {
                continue;
            };

            if !skill_file.exists() {
                continue;
            }

            match parse_skill(&skill_file, SkillTrust::Local) {
                Ok(skill) => {
                    tracing::info!(
                        skill = %skill.name(),
                        version = %skill.manifest.version,
                        "Loaded skill"
                    );
                    skills.insert(skill.name().to_string(), Arc::new(skill));
                    count += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %skill_file.display(),
                        error = %e,
                        "Failed to load skill"
                    );
                }
            }
        }

        Ok(count)
    }

    /// Get a skill by name.
    pub async fn get(&self, name: &str) -> Option<Arc<LoadedSkill>> {
        self.local_skills.read().await.get(name).cloned()
    }

    /// List all local skills.
    pub async fn list_local(&self) -> Vec<Arc<LoadedSkill>> {
        self.local_skills.read().await.values().cloned().collect()
    }

    /// List all available skills (local + network).
    pub async fn list_all(&self) -> Vec<SkillInfo> {
        let mut skills = Vec::new();

        // Local skills
        for skill in self.local_skills.read().await.values() {
            skills.push(SkillInfo {
                name: skill.name().to_string(),
                version: skill.manifest.version.clone(),
                description: skill.description().to_string(),
                trust: skill.trust,
                available: skill.is_available(),
                provider: self.local_peer_id.clone(),
                price: skill.manifest.sharing.price,
            });
        }

        // Network skills
        for (name, announcements) in self.network_skills.read().await.iter() {
            if let Some(best) = announcements.first() {
                skills.push(SkillInfo {
                    name: name.clone(),
                    version: best.version.clone(),
                    description: best.description.clone(),
                    trust: SkillTrust::Network,
                    available: true,
                    provider: best.provider.clone(),
                    price: best.price,
                });
            }
        }

        skills
    }

    /// Register a skill announcement from the network.
    pub async fn register_network_skill(&self, announcement: SkillAnnouncement) {
        let mut network = self.network_skills.write().await;
        let announcements = network.entry(announcement.name.clone()).or_insert_with(Vec::new);

        // Update or add
        if let Some(existing) = announcements.iter_mut().find(|a| a.provider == announcement.provider) {
            *existing = announcement;
        } else {
            announcements.push(announcement);
        }

        // Sort by price
        announcements.sort_by_key(|a| a.price);
    }

    /// Get local skill announcements for sharing.
    pub async fn get_announcements(&self) -> Vec<SkillAnnouncement> {
        let skills = self.local_skills.read().await;
        skills.values()
            .filter(|s| s.manifest.sharing.enabled)
            .map(|s| SkillAnnouncement {
                name: s.name().to_string(),
                version: s.manifest.version.clone(),
                description: s.description().to_string(),
                hash: s.hash.clone(),
                price: s.manifest.sharing.price,
                provider: self.local_peer_id.clone(),
                keywords: s.manifest.activation.keywords.clone(),
                tags: s.manifest.activation.tags.clone(),
            })
            .collect()
    }

    /// Install a skill from content.
    pub async fn install(&self, content: &str, trust: SkillTrust) -> Result<Arc<LoadedSkill>, ParseError> {
        let source = SkillSource::Workspace(self.skills_dir.clone());
        let skill = super::parser::parse_skill_content(content, source, trust)?;
        let name = skill.name().to_string();

        // Save to file
        let skill_file = self.skills_dir.join(format!("{}.md", &name));
        std::fs::write(&skill_file, content)?;

        let skill = Arc::new(skill);
        self.local_skills.write().await.insert(name, skill.clone());

        Ok(skill)
    }

    /// Remove a skill.
    pub async fn remove(&self, name: &str) -> bool {
        let mut skills = self.local_skills.write().await;
        if skills.remove(name).is_some() {
            // Try to remove file
            let skill_file = self.skills_dir.join(format!("{}.md", name));
            let _ = std::fs::remove_file(skill_file);
            true
        } else {
            false
        }
    }
}

/// Skill information for listing.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub trust: SkillTrust,
    pub available: bool,
    pub provider: String,
    pub price: u64,
}

/// Error scanning skills directory.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("IO error: {0}")]
    IoError(String),

    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_registry_creation() {
        let dir = tempdir().unwrap();
        let registry = SkillRegistry::new(
            dir.path().to_path_buf(),
            "test-peer".to_string(),
        ).unwrap();

        let skills = registry.list_local().await;
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn test_install_skill() {
        let dir = tempdir().unwrap();
        let registry = SkillRegistry::new(
            dir.path().to_path_buf(),
            "test-peer".to_string(),
        ).unwrap();

        let content = r#"---
name: test-skill
version: 1.0.0
description: A test skill
---

# Test Skill

This is a test skill.
"#;

        let skill = registry.install(content, SkillTrust::Local).await.unwrap();
        assert_eq!(skill.name(), "test-skill");

        let retrieved = registry.get("test-skill").await;
        assert!(retrieved.is_some());
    }
}
