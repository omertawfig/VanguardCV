//! PII-isolated CV analysis and AI Search Profile extraction.
//!
//! **Strict Privacy Rule:** `doc.pii_header` (name, email, phone, address,
//! social links, custom fields) is NEVER read or included in outbound prompts.
//! Every CV is first passed through [`PrivacyGuard::scrub`], and only scrubbed
//! role titles, skill categories, summaries, and brief experience highlights
//! are sent to the active AI provider.

use std::collections::HashSet;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::contracts::{AiOrchestrator, CvDocument, PrivacyGuard, SectionKind};
use crate::jobs::models::SearchProfile;

/// Compact, strictly PII-free digest of a single CV sent to the AI provider.
#[derive(Debug, Clone, Serialize)]
pub struct ScrubbedCvDigest {
    pub role_titles: Vec<String>,
    pub skills: Vec<String>,
    pub summary: Option<String>,
    pub experience_highlights: Vec<String>,
}

/// Removes any `[REDACTED_PII_*]` placeholders produced by the scrubber so the
/// model never echoes redaction tags as job keywords.
fn strip_redaction_tokens(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("[REDACTED_PII_") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        if let Some(end) = after.find(']') {
            rest = &after[end + 1..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Builds a strictly PII-free digest of one or more saved CVs using the
/// application's [`PrivacyGuard`] scrubber.
pub fn build_scrubbed_digests(
    docs: &[&CvDocument],
    scrubber: &dyn PrivacyGuard,
) -> Vec<ScrubbedCvDigest> {
    let mut digests = Vec::new();
    for doc in docs {
        let Ok(scrubbed) = scrubber.scrub(&doc.body, &doc.pii_header) else {
            continue;
        };
        let mut role_titles = Vec::new();
        let mut seen_roles = HashSet::new();
        let mut skills = Vec::new();
        let mut seen_skills = HashSet::new();
        let mut summary: Option<String> = None;
        let mut experience_highlights = Vec::new();

        for sec in &scrubbed.body.sections {
            if !sec.enabled {
                continue;
            }
            match &sec.kind {
                SectionKind::Summary { text } => {
                    let clean = strip_redaction_tokens(text);
                    if !clean.is_empty() && summary.is_none() {
                        let truncated: String = clean.chars().take(360).collect();
                        summary = Some(truncated);
                    }
                }
                SectionKind::TimelineEntries { entries } => {
                    let is_education = sec.title.to_ascii_lowercase().contains("education");
                    for entry in entries.iter().filter(|e| e.enabled) {
                        if !is_education {
                            if let Some(role) = &entry.secondary_heading {
                                let clean = strip_redaction_tokens(role);
                                if !clean.is_empty()
                                    && seen_roles.insert(clean.to_ascii_lowercase())
                                {
                                    role_titles.push(clean);
                                }
                            }
                            for bullet in entry.bullets.iter().filter(|b| b.enabled).take(2) {
                                let clean = strip_redaction_tokens(&bullet.text);
                                if !clean.is_empty() && experience_highlights.len() < 8 {
                                    let truncated: String = clean.chars().take(180).collect();
                                    experience_highlights.push(truncated);
                                }
                            }
                        }
                    }
                }
                SectionKind::CategorizedInlineList { categories } => {
                    for cat in categories.iter().filter(|c| c.enabled) {
                        for item in &cat.items {
                            let clean = strip_redaction_tokens(item);
                            if !clean.is_empty()
                                && seen_skills.insert(clean.to_ascii_lowercase())
                                && skills.len() < 32
                            {
                                skills.push(clean);
                            }
                        }
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    for item in items.iter().filter(|b| b.enabled).take(2) {
                        let clean = strip_redaction_tokens(&item.text);
                        if !clean.is_empty() && experience_highlights.len() < 8 {
                            let truncated: String = clean.chars().take(180).collect();
                            experience_highlights.push(truncated);
                        }
                    }
                }
            }
        }

        digests.push(ScrubbedCvDigest {
            role_titles,
            skills,
            summary,
            experience_highlights,
        });
    }
    digests
}

/// Builds the system + user prompts for AI search profile generation.
/// Exposed for unit-testing strict zero-PII isolation.
pub fn build_profile_extraction_prompts(digests: &[ScrubbedCvDigest]) -> (String, String) {
    let system_prompt = r#"You are a career search strategist for VanguardCV.
Given anonymized candidate CV highlights (role titles, skills, and brief summaries), generate 2 to 4 concise, realistic job search profiles for querying a live tech/IT job search index.

Rules:
1. Return ONLY valid JSON with this exact shape:
{
  "profiles": [
    {
      "label": "Primary Role",
      "title": "Short realistic job title (2-4 words)",
      "keywords": ["Keyword 1", "Keyword 2", "Keyword 3"]
    }
  ]
}
2. Output between 2 and 4 distinct profiles covering:
   - "Primary Role": the strongest direct match to the candidate's main track.
   - "Adjacent Role": a closely related title employers commonly use for this skill set.
   - "Niche / Stack" (optional 3rd/4th): a specialized or stack-focused role angle.
3. Keep each "title" concise and realistic (2-4 words, e.g. "Security Engineer", "Penetration Tester", "Cloud Security Engineer"). Do NOT concatenate multiple titles with slashes or pipes.
4. Keep "keywords" to 2 to 4 high-signal, single- or two-word technical skills or domains (NOT long phrases, NOT 15 skills).
5. Never invent experience or technologies not grounded in the provided CV data."#
        .to_string();

    let payload_json = serde_json::to_string_pretty(digests).unwrap_or_else(|_| "[]".to_string());
    let user_prompt = format!(
        "Anonymized CV data (zero PII):\n{payload_json}\n\nGenerate 2 to 4 concise job search profiles in JSON."
    );

    (system_prompt, user_prompt)
}

#[derive(Debug, Deserialize)]
struct RawProfilesEnvelope {
    #[serde(default)]
    profiles: Vec<RawProfileItem>,
}

#[derive(Debug, Deserialize)]
struct RawProfileItem {
    #[serde(default)]
    label: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    keywords: Vec<String>,
}

/// Parses the AI provider's JSON response into validated [`SearchProfile`]s,
/// tolerating markdown code fences or prose surrounding the JSON payload.
pub fn parse_search_profiles_json(raw: &str) -> Result<Vec<SearchProfile>> {
    let candidate = extract_json_slice(raw)
        .ok_or_else(|| anyhow!("AI response did not contain a valid JSON object or array"))?;

    let raw_items: Vec<RawProfileItem> =
        if let Ok(env) = serde_json::from_str::<RawProfilesEnvelope>(candidate) {
            env.profiles
        } else if let Ok(arr) = serde_json::from_str::<Vec<RawProfileItem>>(candidate) {
            arr
        } else {
            return Err(anyhow!(
                "Could not deserialize search profiles JSON from AI response"
            ));
        };

    let default_labels = [
        "Primary Role",
        "Adjacent Role",
        "Niche / Stack",
        "Specialty Role",
    ];
    let mut profiles = Vec::new();
    let mut seen_titles = HashSet::new();

    for (idx, item) in raw_items.into_iter().enumerate() {
        let title = item.title.trim().to_string();
        if title.is_empty() || !seen_titles.insert(title.to_ascii_lowercase()) {
            continue;
        }
        let label = if item.label.trim().is_empty() {
            default_labels
                .get(idx)
                .copied()
                .unwrap_or("Target Role")
                .to_string()
        } else {
            item.label.trim().to_string()
        };

        let mut seen_kw = HashSet::new();
        let keywords: Vec<String> = item
            .keywords
            .into_iter()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty() && seen_kw.insert(k.to_ascii_lowercase()))
            .take(4)
            .collect();

        profiles.push(SearchProfile::new(
            format!("profile-{}", profiles.len() + 1),
            label,
            title,
            keywords,
        ));
        if profiles.len() >= 4 {
            break;
        }
    }

    if profiles.is_empty() {
        return Err(anyhow!("AI returned zero usable search profiles"));
    }

    Ok(profiles)
}

fn extract_json_slice(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    // Strip ```json ... ``` or ``` ... ``` fences if present.
    let unfenced = if let Some(after_open) = trimmed.strip_prefix("```") {
        let after_lang = after_open
            .find('\n')
            .map(|i| &after_open[i + 1..])
            .unwrap_or(after_open);
        after_lang
            .rfind("```")
            .map(|i| &after_lang[..i])
            .unwrap_or(after_lang)
            .trim()
    } else {
        trimmed
    };

    if (unfenced.starts_with('{') && unfenced.ends_with('}'))
        || (unfenced.starts_with('[') && unfenced.ends_with(']'))
    {
        return Some(unfenced);
    }

    if let (Some(start), Some(end)) = (unfenced.find('{'), unfenced.rfind('}'))
        && start < end
    {
        return Some(&unfenced[start..=end]);
    }
    if let (Some(start), Some(end)) = (unfenced.find('['), unfenced.rfind(']'))
        && start < end
    {
        return Some(&unfenced[start..=end]);
    }
    None
}

/// Deterministic, zero-token fallback profile generator used when AI parsing
/// fails or when no AI provider is configured yet.
pub fn extract_fallback_profiles(
    docs: &[&CvDocument],
    scrubber: &dyn PrivacyGuard,
) -> Vec<SearchProfile> {
    let digests = build_scrubbed_digests(docs, scrubber);
    let mut role_titles: Vec<String> = Vec::new();
    let mut seen_roles = HashSet::new();
    let mut skills: Vec<String> = Vec::new();
    let mut seen_skills = HashSet::new();

    for digest in &digests {
        for role in &digest.role_titles {
            if seen_roles.insert(role.to_ascii_lowercase()) {
                role_titles.push(role.clone());
            }
        }
        for skill in &digest.skills {
            if seen_skills.insert(skill.to_ascii_lowercase()) {
                skills.push(skill.clone());
            }
        }
    }

    let mut profiles = Vec::new();
    let labels = ["Primary Role", "Adjacent Role", "Niche / Stack"];

    for (idx, role) in role_titles.iter().take(3).enumerate() {
        let chunk_start = (idx * 2).min(skills.len());
        let mut kws: Vec<String> = skills.iter().skip(chunk_start).take(3).cloned().collect();
        if kws.is_empty() {
            kws = skills.iter().take(3).cloned().collect();
        }
        profiles.push(SearchProfile::new(
            format!("fallback-{}", idx + 1),
            labels.get(idx).copied().unwrap_or("Target Role"),
            role.clone(),
            kws,
        ));
    }

    // If CV had only 1 role title but multiple skills, synthesize an adjacent stack profile.
    if profiles.len() == 1 && skills.len() >= 2 {
        let stack_title = format!("{} Engineer", skills[0]);
        profiles.push(SearchProfile::new(
            "fallback-2",
            "Niche / Stack",
            stack_title,
            skills.iter().take(3).cloned().collect(),
        ));
    }

    if profiles.is_empty() {
        let fallback_title = skills
            .first()
            .map(|s| format!("{s} Engineer"))
            .unwrap_or_else(|| "Software Engineer".to_string());
        profiles.push(SearchProfile::new(
            "fallback-1",
            "Primary Role",
            fallback_title,
            skills.into_iter().take(3).collect(),
        ));
    }

    profiles
}

/// Calls the active [`AiOrchestrator`] on strictly PII-stripped CV digests to
/// produce 2–4 concise [`SearchProfile`]s.
///
/// - Propagates API / rate-limit / auth errors directly so the UI can clearly
///   notify the user (e.g., "API limit reached").
/// - Falls back safely to deterministic local extraction only when the AI
///   returned unparseable output, returning `(profiles, used_fallback)`.
pub async fn generate_search_profiles_with_ai(
    ai: &dyn AiOrchestrator,
    docs: &[&CvDocument],
    scrubber: &dyn PrivacyGuard,
) -> Result<(Vec<SearchProfile>, bool)> {
    let digests = build_scrubbed_digests(docs, scrubber);
    let (system_prompt, user_prompt) = build_profile_extraction_prompts(&digests);
    let raw = ai
        .complete_structured_prompt(system_prompt, user_prompt)
        .await
        ?;

    match parse_search_profiles_json(&raw) {
        Ok(profiles) => Ok((profiles, false)),
        Err(err) => {
            tracing::warn!(
                "AI search profile JSON parse failed ({err}); using deterministic fallback"
            );
            Ok((extract_fallback_profiles(docs, scrubber), true))
        }
    }
}
