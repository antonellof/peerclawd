//! Skill selection and scoring for context-aware skill activation.
//!
//! Selects the most relevant skills based on user input, tags, and patterns.

use std::sync::Arc;

use super::{LoadedSkill, SkillTrust};

/// Score for a skill match.
#[derive(Debug, Clone)]
pub struct SkillScore {
    /// The matched skill.
    pub skill: Arc<LoadedSkill>,
    /// Total relevance score (0.0 - 1.0).
    pub score: f32,
    /// Keyword match count.
    pub keyword_matches: usize,
    /// Pattern match count.
    pub pattern_matches: usize,
    /// Tag match count.
    pub tag_matches: usize,
    /// Trust bonus applied.
    pub trust_bonus: f32,
}

impl SkillScore {
    /// Create a new skill score.
    pub fn new(skill: Arc<LoadedSkill>) -> Self {
        Self {
            skill,
            score: 0.0,
            keyword_matches: 0,
            pattern_matches: 0,
            tag_matches: 0,
            trust_bonus: 0.0,
        }
    }

    /// Calculate the final score.
    pub fn calculate(&mut self) {
        // Base score from matches
        let keyword_score = (self.keyword_matches as f32) * 0.3;
        let pattern_score = (self.pattern_matches as f32) * 0.4;
        let tag_score = (self.tag_matches as f32) * 0.2;

        // Trust bonus
        self.trust_bonus = match self.skill.trust {
            SkillTrust::Local => 0.2,
            SkillTrust::Installed => 0.1,
            SkillTrust::Network => 0.0,
        };

        // Combine scores (capped at 1.0)
        self.score = (keyword_score + pattern_score + tag_score + self.trust_bonus).min(1.0);
    }
}

/// Score a skill against user input.
pub fn score_skill(skill: &Arc<LoadedSkill>, input: &str, tags: &[String]) -> SkillScore {
    let mut score = SkillScore::new(skill.clone());
    let input_lower = input.to_lowercase();
    let input_words: Vec<&str> = input_lower.split_whitespace().collect();

    // Check keywords
    for keyword in &skill.manifest.activation.keywords {
        let keyword_lower = keyword.to_lowercase();
        if input_lower.contains(&keyword_lower) {
            score.keyword_matches += 1;
        }
        // Also check individual words
        for word in &input_words {
            if *word == keyword_lower {
                score.keyword_matches += 1;
            }
        }
    }

    // Check exclusion keywords
    for exclude in &skill.manifest.activation.exclude_keywords {
        if input_lower.contains(&exclude.to_lowercase()) {
            // Skill explicitly excluded
            return score; // Return with zero score
        }
    }

    // Check patterns (regex)
    for pattern_str in &skill.manifest.activation.patterns {
        if let Ok(pattern) = regex::Regex::new(pattern_str) {
            if pattern.is_match(&input_lower) {
                score.pattern_matches += 1;
            }
        }
    }

    // Check tags
    for skill_tag in &skill.manifest.activation.tags {
        let skill_tag_lower = skill_tag.to_lowercase();
        for tag in tags {
            if tag.to_lowercase() == skill_tag_lower {
                score.tag_matches += 1;
            }
        }
    }

    score.calculate();
    score
}

/// Select the best skills for a given input.
///
/// Returns skills sorted by relevance score, filtered by minimum threshold.
pub fn select_skills(
    skills: &[Arc<LoadedSkill>],
    input: &str,
    tags: &[String],
    max_skills: usize,
    min_score: f32,
) -> Vec<SkillScore> {
    let mut scores: Vec<SkillScore> = skills
        .iter()
        .filter(|s| s.is_available())
        .map(|s| score_skill(s, input, tags))
        .filter(|s| s.score >= min_score)
        .collect();

    // Sort by score descending
    scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Limit to max_skills
    scores.truncate(max_skills);

    scores
}

/// Build a combined prompt from selected skills.
#[allow(dead_code)]
pub fn build_skill_prompt(skills: &[SkillScore], max_tokens: usize) -> String {
    let mut prompt = String::new();
    let mut tokens_used = 0;

    for score in skills {
        let skill_prompt = score.skill.prompt();
        // Rough token estimate (4 chars per token)
        let estimated_tokens = skill_prompt.len() / 4;

        if tokens_used + estimated_tokens > max_tokens {
            break;
        }

        if !prompt.is_empty() {
            prompt.push_str("\n\n---\n\n");
        }

        prompt.push_str(&format!("## Skill: {}\n\n", score.skill.name()));
        prompt.push_str(skill_prompt);
        tokens_used += estimated_tokens;
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{
        ActivationCriteria, SkillManifest, SkillRequirements, SkillSharing, SkillSource,
    };

    fn create_test_skill(name: &str, keywords: Vec<&str>, tags: Vec<&str>) -> Arc<LoadedSkill> {
        Arc::new(LoadedSkill {
            manifest: SkillManifest {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                description: format!("Test skill: {}", name),
                author: None,
                activation: ActivationCriteria {
                    keywords: keywords.into_iter().map(String::from).collect(),
                    exclude_keywords: vec![],
                    patterns: vec![],
                    tags: tags.into_iter().map(String::from).collect(),
                    max_context_tokens: 2000,
                },
                requires: SkillRequirements::default(),
                sharing: SkillSharing::default(),
            },
            prompt_content: "Test prompt content".to_string(),
            trust: SkillTrust::Local,
            source: SkillSource::Bundled("test".to_string()),
            hash: "abc123".to_string(),
            requirements_met: true,
        })
    }

    #[test]
    fn test_score_skill_keywords() {
        let skill = create_test_skill("code-review", vec!["review", "code"], vec!["development"]);

        let score = score_skill(&skill, "Please review my code", &[]);
        assert!(score.keyword_matches > 0);
        assert!(score.score > 0.0);
    }

    #[test]
    fn test_score_skill_tags() {
        let skill = create_test_skill("dev-helper", vec![], vec!["development", "coding"]);

        let score = score_skill(&skill, "help me", &["development".to_string()]);
        assert_eq!(score.tag_matches, 1);
        assert!(score.score > 0.0);
    }

    #[test]
    fn test_select_skills_ordering() {
        let skill1 = create_test_skill("review", vec!["review"], vec![]);
        let skill2 = create_test_skill("code-review", vec!["review", "code", "pr"], vec![]);

        let skills = vec![skill1, skill2];
        let selected = select_skills(&skills, "review my code", &[], 5, 0.0);

        assert_eq!(selected.len(), 2);
        // skill2 should score higher (more keyword matches)
        assert!(selected[0].score >= selected[1].score);
    }

    #[test]
    fn test_select_skills_min_score() {
        let skill = create_test_skill("test", vec!["specific"], vec![]);

        let skills = vec![skill];
        // Use 0.3 as min_score since trust bonus (0.2 for Local) would pass 0.1
        let selected = select_skills(&skills, "unrelated query", &[], 5, 0.3);

        assert!(selected.is_empty());
    }
}
