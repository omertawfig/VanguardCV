//! Concrete `FileSkillRegistry` implementing [`crate::contracts::SkillRegistry`].
//!
//! Loads bundled TOML skills from `skills/*.toml` and user skills from
//! `~/.config/vanguard-cv/skills/`. Supports TOML files as well as Markdown
//! files with TOML/YAML frontmatter (`+++` or `---`). Supports hot reload via
//! `notify`, plus deleting both user and bundled default skills (with optional
//! restore).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Result, anyhow, bail};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher, recommended_watcher};

use crate::contracts::{SkillManifest, SkillRegistry};

/// Filesystem-backed skill registry with hot-reloading support.
pub struct FileSkillRegistry {
    bundled_dir: PathBuf,
    user_dir: PathBuf,
    skills: RwLock<Vec<SkillManifest>>,
    disabled: RwLock<HashSet<String>>,
    disabled_file: PathBuf,
    deleted_bundled: RwLock<HashSet<String>>,
    deleted_bundled_file: PathBuf,
}

impl FileSkillRegistry {
    /// Creates a new registry with default locations:
    /// - Bundled: `./skills` (or env `VANGUARDCV_SKILLS_DIR`)
    /// - User: `~/.config/vanguard-cv/skills`
    pub fn new() -> Result<Self> {
        let bundled = std::env::var("VANGUARDCV_SKILLS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let local = PathBuf::from("skills");
                if local.exists() {
                    return local;
                }
                if let Some(exe_skills) = std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|dir| dir.join("skills")))
                    && exe_skills.exists()
                {
                    return exe_skills;
                }
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills")
            });

        let user = dirs::config_dir()
            .map(|d| d.join("vanguard-cv").join("skills"))
            .unwrap_or_else(|| PathBuf::from(".config/vanguard-cv/skills"));

        Self::with_paths(bundled, user)
    }

    /// Creates a new registry with explicit paths (useful for testing and configuration).
    pub fn with_paths(bundled_dir: PathBuf, user_dir: PathBuf) -> Result<Self> {
        let disabled_file = user_dir.join(".disabled.json");
        let disabled = load_id_set(&disabled_file);
        let deleted_bundled_file = user_dir.join(".deleted_bundled.json");
        let deleted_bundled = load_id_set(&deleted_bundled_file);
        let registry = Self {
            bundled_dir,
            user_dir,
            skills: RwLock::new(Vec::new()),
            disabled: RwLock::new(disabled),
            disabled_file,
            deleted_bundled: RwLock::new(deleted_bundled),
            deleted_bundled_file,
        };
        // Initial load
        registry.reload()?;
        Ok(registry)
    }

    /// Starts a `notify` watcher on the user skills directory and bundled directory.
    ///
    /// The watcher runs on a background thread managed by `notify` and triggers
    /// [`SkillRegistry::reload`] whenever files are created, modified, or removed.
    pub fn start_watcher(self: &Arc<Self>) -> Result<RecommendedWatcher> {
        // Ensure user directory exists so it can be watched
        if !self.user_dir.exists() {
            let _ = std::fs::create_dir_all(&self.user_dir);
        }

        let registry = Arc::clone(self);
        let mut watcher = recommended_watcher(move |res: notify::Result<Event>| match res {
            Ok(event) => {
                tracing::debug!("Skills directory file event: {:?}", event);
                if let Err(err) = registry.reload() {
                    tracing::warn!("Failed to reload skills after file event: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("Skills watcher error: {err}");
            }
        })?;

        if self.user_dir.exists() {
            let _ = watcher.watch(&self.user_dir, RecursiveMode::Recursive);
        }
        if self.bundled_dir.exists() {
            let _ = watcher.watch(&self.bundled_dir, RecursiveMode::Recursive);
        }

        Ok(watcher)
    }

    /// Helper to get a copy of the bundled dir path.
    pub fn bundled_dir(&self) -> &Path {
        &self.bundled_dir
    }
}

impl Default for FileSkillRegistry {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            bundled_dir: PathBuf::from("skills"),
            user_dir: PathBuf::from(".config/vanguard-cv/skills"),
            skills: RwLock::new(Vec::new()),
            disabled: RwLock::new(HashSet::new()),
            disabled_file: PathBuf::from(".config/vanguard-cv/skills/.disabled.json"),
            deleted_bundled: RwLock::new(HashSet::new()),
            deleted_bundled_file: PathBuf::from(".config/vanguard-cv/skills/.deleted_bundled.json"),
        })
    }
}

impl SkillRegistry for FileSkillRegistry {
    fn list(&self) -> Vec<SkillManifest> {
        self.skills.read().map(|s| s.clone()).unwrap_or_default()
    }

    fn get(&self, id: &str) -> Option<SkillManifest> {
        self.skills
            .read()
            .ok()?
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    fn reload(&self) -> Result<Vec<SkillManifest>> {
        // Ensure user skills directory exists
        if !self.user_dir.exists()
            && let Err(err) = std::fs::create_dir_all(&self.user_dir)
        {
            tracing::warn!(
                "Could not create user skills directory {}: {}",
                self.user_dir.display(),
                err
            );
        }

        let deleted_bundled = self
            .deleted_bundled
            .read()
            .map(|set| set.clone())
            .unwrap_or_default();

        let mut map: BTreeMap<String, SkillManifest> = BTreeMap::new();

        // 1. Load bundled skills (excluding any bundled skills deleted by the user)
        if self.bundled_dir.exists() && self.bundled_dir.is_dir() {
            let mut bundled_map = BTreeMap::new();
            load_directory(&self.bundled_dir, &mut bundled_map);
            for (id, manifest) in bundled_map {
                if !deleted_bundled.contains(&id) {
                    map.insert(id, manifest);
                }
            }
        }

        // 2. Load user skills (can override or supplement bundled skills)
        if self.user_dir.exists() && self.user_dir.is_dir() {
            load_directory(&self.user_dir, &mut map);
        }

        let mut loaded: Vec<SkillManifest> = map.into_values().collect();
        loaded.sort_by(|a, b| a.name.cmp(&b.name));

        let disabled = self
            .disabled
            .read()
            .map(|set| set.clone())
            .unwrap_or_default();
        for skill in &mut loaded {
            skill.enabled = !disabled.contains(&skill.id);
        }

        if let Ok(mut lock) = self.skills.write() {
            *lock = loaded.clone();
        }

        Ok(loaded)
    }

    fn compose_system_prompt(&self, skill_ids: &[String]) -> Result<String> {
        let mut prompt = String::new();
        let skills = self.list();

        for id in skill_ids {
            let Some(skill) = skills.iter().find(|s| s.id == *id) else {
                tracing::debug!("Skill ID '{}' not found in registry; skipping", id);
                continue;
            };
            if !skill.enabled {
                tracing::debug!("Skill '{}' is disabled; skipping", id);
                continue;
            }

            prompt.push_str(&format!("=== SKILL: {} ({}) ===\n", skill.name, skill.id));
            if !skill.category.is_empty() {
                prompt.push_str(&format!("Category: {}\n", skill.category));
            }
            if let Some(desc) = &skill.description {
                prompt.push_str(&format!("Description: {}\n", desc));
            }
            if !skill.rules.is_empty() {
                prompt.push_str("Rules:\n");
                for (k, v) in &skill.rules {
                    prompt.push_str(&format!("- {}: {}\n", k, v));
                }
            }
            let directives = skill.prompt_injection.system_directives.trim();
            if !directives.is_empty() {
                prompt.push_str("Directives:\n");
                prompt.push_str(directives);
                prompt.push_str("\n\n");
            }
        }

        Ok(prompt)
    }

    fn skills_dir(&self) -> PathBuf {
        self.user_dir.clone()
    }

    fn set_skill_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let exists = self
            .skills
            .read()
            .map_err(|_| anyhow!("skill registry lock poisoned"))?
            .iter()
            .any(|skill| skill.id == id);
        if !exists {
            bail!("unknown skill '{id}'");
        }

        {
            let mut disabled = self
                .disabled
                .write()
                .map_err(|_| anyhow!("skill registry lock poisoned"))?;
            if enabled {
                disabled.remove(id);
            } else {
                disabled.insert(id.to_string());
            }
            save_id_set(&self.disabled_file, &disabled)?;
        }

        if let Ok(mut skills) = self.skills.write()
            && let Some(skill) = skills.iter_mut().find(|skill| skill.id == id)
        {
            skill.enabled = enabled;
        }
        Ok(())
    }

    fn import_skill(&self, source: &Path) -> Result<SkillManifest> {
        let extension = source
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension != "md" && extension != "toml" {
            bail!("skill files must be Markdown (.md) or TOML (.toml)");
        }

        let content = fs::read_to_string(source)?;
        let manifest = if extension == "md" {
            parse_markdown_manifest(&content)
                .or_else(|_| plain_markdown_manifest(source, &content))?
        } else {
            parse_toml_manifest(&content)?
        };

        let raw_id = if manifest.id.trim().is_empty() {
            source
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("imported-skill")
                .to_string()
        } else {
            manifest.id.clone()
        };
        let id = slugify(&raw_id);
        if id.is_empty() {
            bail!(
                "could not derive a valid skill name from {}",
                source.display()
            );
        }

        fs::create_dir_all(&self.user_dir)?;

        if extension == "md" {
            let body = strip_frontmatter(&content);
            let description = manifest
                .description
                .as_deref()
                .map(str::trim)
                .filter(|description| !description.is_empty())
                .map(str::to_string)
                .or_else(|| first_meaningful_line(body))
                .unwrap_or_else(|| manifest.name.clone());
            let name = if manifest.name.trim().is_empty() {
                id.clone()
            } else {
                manifest.name.clone()
            };
            let dir = self.user_dir.join(&id);
            fs::create_dir_all(&dir)?;
            let mut normalized = format!(
                "---\nname: {}\ndescription: {}\n",
                yaml_escape(&name),
                yaml_escape(&description)
            );
            if !manifest.category.trim().is_empty() {
                normalized.push_str(&format!("category: {}\n", yaml_escape(&manifest.category)));
            }
            if !manifest.version.trim().is_empty() {
                normalized.push_str(&format!("version: {}\n", yaml_escape(&manifest.version)));
            }
            if manifest.default_enabled {
                normalized.push_str("default_enabled: true\n");
            }
            normalized.push_str(&format!("---\n\n{}\n", body.trim()));
            fs::write(dir.join("SKILL.md"), normalized)?;
        } else {
            fs::write(self.user_dir.join(format!("{id}.toml")), content)?;
        }

        // If the user explicitly re-imports a skill whose id was previously hidden, unhide it.
        if let Ok(mut deleted) = self.deleted_bundled.write()
            && deleted.remove(&id)
        {
            let _ = save_id_set(&self.deleted_bundled_file, &deleted);
        }

        self.reload()?;
        self.get(&id)
            .or_else(|| self.get(&manifest.id))
            .ok_or_else(|| anyhow!("skill '{id}' was written but could not be reloaded"))
    }

    fn delete_skill(&self, id: &str) -> Result<()> {
        let manifest = self
            .get(id)
            .ok_or_else(|| anyhow!("unknown skill '{id}'"))?;
        let source = manifest
            .source_path
            .ok_or_else(|| anyhow!("skill '{id}' has no source file"))?;

        let user_root = self
            .user_dir
            .canonicalize()
            .unwrap_or_else(|_| self.user_dir.clone());
        let canonical = source
            .canonicalize()
            .map_err(|err| anyhow!("cannot resolve {}: {err}", source.display()))?;

        if canonical.starts_with(&user_root) {
            fs::remove_file(&canonical)?;
            if let Some(parent) = canonical.parent()
                && parent != user_root
                && parent.starts_with(&user_root)
            {
                let _ = fs::remove_dir(parent);
            }
        }

        // If a bundled skill with this ID exists, record a persistent deletion marker so
        // it stays deleted across reloads and app restarts without mutating bundled assets.
        if self.bundled_dir.exists() && self.bundled_dir.is_dir() {
            let mut bundled_map = BTreeMap::new();
            load_directory(&self.bundled_dir, &mut bundled_map);
            if bundled_map.contains_key(id) {
                let mut deleted = self
                    .deleted_bundled
                    .write()
                    .map_err(|_| anyhow!("skill registry lock poisoned"))?;
                deleted.insert(id.to_string());
                save_id_set(&self.deleted_bundled_file, &deleted)?;
            }
        }

        {
            let mut disabled = self
                .disabled
                .write()
                .map_err(|_| anyhow!("skill registry lock poisoned"))?;
            if disabled.remove(id) {
                save_id_set(&self.disabled_file, &disabled)?;
            }
        }

        self.reload()?;
        Ok(())
    }

    fn restore_bundled_skills(&self) -> Result<usize> {
        let count = {
            let mut deleted = self
                .deleted_bundled
                .write()
                .map_err(|_| anyhow!("skill registry lock poisoned"))?;
            let count = deleted.len();
            deleted.clear();
            save_id_set(&self.deleted_bundled_file, &deleted)?;
            count
        };
        self.reload()?;
        Ok(count)
    }

    fn has_deleted_bundled_skills(&self) -> bool {
        self.deleted_bundled
            .read()
            .map(|set| !set.is_empty())
            .unwrap_or(false)
    }

    fn reset_to_defaults(&self) -> Result<()> {
        if self.user_dir.exists() {
            for entry in fs::read_dir(&self.user_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        {
            let mut disabled = self
                .disabled
                .write()
                .map_err(|_| anyhow!("skill registry lock poisoned"))?;
            disabled.clear();
        }
        {
            let mut deleted = self
                .deleted_bundled
                .write()
                .map_err(|_| anyhow!("skill registry lock poisoned"))?;
            deleted.clear();
        }
        self.reload()?;
        Ok(())
    }
}

fn load_id_set(path: &Path) -> HashSet<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .map(|ids| ids.into_iter().collect())
        .unwrap_or_default()
}

fn save_id_set(path: &Path, ids: &HashSet<String>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut sorted: Vec<&String> = ids.iter().collect();
    sorted.sort();
    fs::write(path, serde_json::to_vec_pretty(&sorted)?)?;
    Ok(())
}

fn slugify(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            separator = false;
        } else if !separator && !slug.is_empty() {
            slug.push('-');
            separator = true;
        }
    }
    slug.trim_matches('-').chars().take(64).collect()
}

fn yaml_escape(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\r', "").replace('\n', " "))
}

fn plain_markdown_manifest(path: &Path, content: &str) -> Result<SkillManifest> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("imported-skill");
    let name = stem.replace(['_', '-'], " ");
    let description = first_meaningful_line(content).unwrap_or_else(|| name.clone());
    Ok(SkillManifest {
        id: slugify(stem),
        name,
        description: Some(description),
        enabled: true,
        ..SkillManifest::default()
    })
}

fn strip_frontmatter(content: &str) -> &str {
    let trimmed = content.trim_start();
    for delimiter in ["+++", "---"] {
        if let Some(rest) = trimmed.strip_prefix(delimiter)
            && let Some(end) = rest.find(delimiter)
        {
            return rest[end + delimiter.len()..].trim_start_matches(['\r', '\n']);
        }
    }
    content
}

fn first_meaningful_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("---")
                && !line.starts_with("+++")
        })
        .map(|line| line.trim_start_matches(['-', '*', ' ']).trim().to_string())
        .filter(|line| !line.is_empty())
}

/// Recursively or flatly reads `.toml` and `.md` files in a directory and inserts valid skills.
/// Errors on individual files are logged gracefully so one corrupt file never halts the registry.
fn load_directory(dir: &Path, map: &mut BTreeMap<String, SkillManifest>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!("Failed to read skills directory {}: {}", dir.display(), err);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_skill_file = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("toml") || ext.eq_ignore_ascii_case("md"))
            .unwrap_or(false);

        if !is_skill_file {
            continue;
        }

        // Documentation lives alongside the bundled manifests; it is not a skill.
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("readme.md"))
        {
            continue;
        }

        match parse_skill_file(&path) {
            Ok(manifest) => {
                map.insert(manifest.id.clone(), manifest);
            }
            Err(err) => {
                tracing::warn!(
                    "Skipping invalid skill manifest at {}: {:#}",
                    path.display(),
                    err
                );
            }
        }
    }

    // omp-native layout: `<skills-root>/<skill-name>/SKILL.md` (one level deep).
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_file = path.join("SKILL.md");
            if !skill_file.is_file() {
                continue;
            }
            match parse_skill_file(&skill_file) {
                Ok(manifest) => {
                    map.insert(manifest.id.clone(), manifest);
                }
                Err(err) => tracing::warn!(
                    "Skipping invalid SKILL.md at {}: {:#}",
                    skill_file.display(),
                    err
                ),
            }
        }
    }
}

/// Parses a skill file from disk, determining whether it is TOML or Markdown frontmatter.
pub fn parse_skill_file(path: &Path) -> Result<SkillManifest> {
    let content = std::fs::read_to_string(path)?;
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let mut manifest = if extension.eq_ignore_ascii_case("md") {
        parse_markdown_manifest(&content)?
    } else {
        // Default to TOML parse, but fallback to markdown frontmatter if TOML fails
        parse_toml_manifest(&content).or_else(|_| parse_markdown_manifest(&content))?
    };

    manifest.source_path = Some(path.to_path_buf());

    if manifest.id.trim().is_empty() {
        let fallback = if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
        {
            path.parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
        } else {
            path.file_stem().and_then(|stem| stem.to_str())
        };
        if let Some(fallback) = fallback {
            manifest.id = fallback.to_string();
        }
    }
    if manifest.name.trim().is_empty() {
        manifest.name = manifest.id.clone();
    }

    Ok(manifest)
}

/// Parses a plain TOML skill manifest.
pub fn parse_toml_manifest(content: &str) -> Result<SkillManifest> {
    let manifest: SkillManifest = toml::from_str(content)?;
    Ok(manifest)
}

/// Parses a Markdown skill file with TOML frontmatter delimited by `+++` or `---`.
pub fn parse_markdown_manifest(content: &str) -> Result<SkillManifest> {
    let trimmed = content.trim_start();
    let delimiter = if trimmed.starts_with("+++") {
        "+++"
    } else if trimmed.starts_with("---") {
        "---"
    } else {
        bail!("Markdown skill missing frontmatter delimiter ('+++' or '---')");
    };

    let after_start = &trimmed[delimiter.len()..];
    let after_start = after_start.strip_prefix('\r').unwrap_or(after_start);
    let after_start = after_start.strip_prefix('\n').unwrap_or(after_start);

    let end_idx = after_start
        .find(delimiter)
        .ok_or_else(|| anyhow!("Unclosed frontmatter delimiter '{delimiter}' in markdown skill"))?;

    let frontmatter_str = &after_start[..end_idx];
    let body_str = after_start[end_idx + delimiter.len()..].trim();

    let mut manifest: SkillManifest = toml::from_str(frontmatter_str)
        .or_else(|_| parse_yaml_frontmatter(frontmatter_str))
        .map_err(|e| anyhow!("Failed to parse skill frontmatter: {e}"))?;

    // If frontmatter didn't specify system directives, or if markdown body has text,
    // combine or populate them.
    if manifest
        .prompt_injection
        .system_directives
        .trim()
        .is_empty()
    {
        manifest.prompt_injection.system_directives = body_str.to_string();
    } else if !body_str.is_empty() {
        manifest.prompt_injection.system_directives.push_str("\n\n");
        manifest
            .prompt_injection
            .system_directives
            .push_str(body_str);
    }

    Ok(manifest)
}

/// Minimal YAML frontmatter parser for `SKILL.md` fields. It supports the
/// scalar keys used by omp/Agent Skills (`name`, `description`, `enabled`,
/// `default_enabled`, `category`, `version`, `id`) and ignores lists/unknown
/// keys without a YAML dependency.
fn parse_yaml_frontmatter(content: &str) -> Result<SkillManifest> {
    let mut manifest = SkillManifest {
        enabled: true,
        ..SkillManifest::default()
    };
    let mut recognized = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        match key {
            "id" => {
                manifest.id = value;
                recognized = true;
            }
            "name" => {
                manifest.name = value;
                recognized = true;
            }
            "description" => {
                manifest.description = Some(value);
                recognized = true;
            }
            "category" => {
                manifest.category = value;
                recognized = true;
            }
            "version" => {
                manifest.version = value;
                recognized = true;
            }
            "default_enabled" | "default-enabled" => {
                manifest.default_enabled = value.eq_ignore_ascii_case("true");
                recognized = true;
            }
            "enabled" => {
                manifest.enabled = !value.eq_ignore_ascii_case("false");
                recognized = true;
            }
            _ => {}
        }
    }

    if !recognized {
        bail!("frontmatter contains no recognized skill fields");
    }
    if manifest.id.trim().is_empty() {
        manifest.id = slugify(&manifest.name);
    }
    if manifest.name.trim().is_empty() {
        manifest.name = manifest.id.clone();
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_toml_manifest() {
        let toml_str = r#"
id = "test_skill"
name = "Test Skill"
category = "Testing"
version = "2.0.0"
description = "A skill used in unit tests."
default_enabled = true

[rules]
strict = true
max_items = 5

[prompt_injection]
system_directives = "- Always write clear unit tests."
"#;

        let manifest = parse_toml_manifest(toml_str).expect("should parse TOML");
        assert_eq!(manifest.id, "test_skill");
        assert_eq!(manifest.name, "Test Skill");
        assert_eq!(manifest.category, "Testing");
        assert_eq!(manifest.version, "2.0.0");
        assert_eq!(
            manifest.description.as_deref(),
            Some("A skill used in unit tests.")
        );
        assert!(manifest.default_enabled);
        assert_eq!(
            manifest.rules.get("strict").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            manifest.prompt_injection.system_directives,
            "- Always write clear unit tests."
        );
    }

    #[test]
    fn test_parse_markdown_manifest_plus_delimiter() {
        let md_str = r#"+++
id = "md_skill_plus"
name = "Markdown Plus Skill"
category = "Formatting"
version = "1.0.0"
+++

# Markdown Body Directives

- Bullet directive 1
- Bullet directive 2
"#;

        let manifest = parse_markdown_manifest(md_str).expect("should parse plus delimiter");
        assert_eq!(manifest.id, "md_skill_plus");
        assert_eq!(manifest.name, "Markdown Plus Skill");
        assert!(
            manifest
                .prompt_injection
                .system_directives
                .contains("Bullet directive 1")
        );
    }

    #[test]
    fn test_parse_markdown_manifest_dash_delimiter() {
        let md_str = r#"---
id = "md_skill_dash"
name = "Markdown Dash Skill"
version = "1.1.0"
---

- Directive from dash body.
"#;

        let manifest = parse_markdown_manifest(md_str).expect("should parse dash delimiter");
        assert_eq!(manifest.id, "md_skill_dash");
        assert_eq!(manifest.name, "Markdown Dash Skill");
        assert!(
            manifest
                .prompt_injection
                .system_directives
                .contains("Directive from dash body.")
        );
    }

    #[test]
    fn test_bundled_skills_exist_and_load() {
        let bundled_path = PathBuf::from("skills");
        assert!(
            bundled_path.exists(),
            "skills/ directory must exist in repo root"
        );

        let expected_skills = [
            "ats_classic_optimizer",
            "xyz_bullet_enhancer",
            "keyword_aligner",
            "page_compressor",
        ];

        for skill_id in expected_skills {
            let file_path = bundled_path.join(format!("{skill_id}.toml"));
            assert!(
                file_path.exists(),
                "Bundled skill file {:?} should exist",
                file_path
            );
            let manifest = parse_skill_file(&file_path)
                .unwrap_or_else(|e| panic!("Failed to parse {file_path:?}: {e}"));
            assert_eq!(manifest.id, skill_id);
            assert!(!manifest.prompt_injection.system_directives.is_empty());
        }
    }

    #[test]
    fn test_registry_load_and_bad_file_skipped() {
        let temp = tempdir().expect("tempdir");
        let bundled = temp.path().join("bundled");
        let user = temp.path().join("user");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&user).unwrap();

        // 1 valid bundled skill
        std::fs::write(
            bundled.join("skill_a.toml"),
            r#"
id = "skill_a"
name = "Skill A"
[prompt_injection]
system_directives = "Directive A"
"#,
        )
        .unwrap();

        // 1 corrupted file
        std::fs::write(bundled.join("bad_skill.toml"), "INVALID TOML {{{ [[[").unwrap();

        // 1 valid user skill
        std::fs::write(
            user.join("skill_u.toml"),
            r#"
id = "skill_u"
name = "User Skill"
[prompt_injection]
system_directives = "User Directive"
"#,
        )
        .unwrap();

        let registry = FileSkillRegistry::with_paths(bundled, user).expect("registry with paths");
        let list = registry.list();

        assert_eq!(list.len(), 2, "bad file must be skipped gracefully");
        assert!(registry.get("skill_a").is_some());
        assert!(registry.get("skill_u").is_some());
        assert!(registry.get("bad_skill").is_none());
    }

    #[test]
    fn test_compose_system_prompt() {
        let temp = tempdir().expect("tempdir");
        let bundled = temp.path().join("bundled");
        let user = temp.path().join("user");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&user).unwrap();

        std::fs::write(
            bundled.join("skill_1.toml"),
            r#"
id = "skill_1"
name = "Skill One"
category = "First Category"
[rules]
enforce = true
[prompt_injection]
system_directives = "Directive 1 body"
"#,
        )
        .unwrap();

        let registry = FileSkillRegistry::with_paths(bundled, user).expect("registry");
        let prompt = registry
            .compose_system_prompt(&["skill_1".to_string(), "non_existent".to_string()])
            .expect("should compose prompt");

        assert!(prompt.contains("Skill One"));
        assert!(prompt.contains("First Category"));
        assert!(prompt.contains("enforce: true"));
        assert!(prompt.contains("Directive 1 body"));
    }

    #[test]
    fn test_yaml_skill_md_parses_with_omp_frontmatter() {
        let md = r#"---
name: PDF Toolkit
description: Extract, merge, and inspect PDF files.
enabled: true
---

# PDF Toolkit

Use the local PDF tooling.
"#;
        let manifest = parse_markdown_manifest(md).expect("YAML frontmatter should parse");
        assert_eq!(manifest.id, "pdf-toolkit");
        assert_eq!(manifest.name, "PDF Toolkit");
        assert_eq!(
            manifest.description.as_deref(),
            Some("Extract, merge, and inspect PDF files.")
        );
        assert!(manifest.enabled);
        assert!(
            manifest
                .prompt_injection
                .system_directives
                .contains("local PDF tooling")
        );
    }

    #[test]
    fn test_import_disable_reenable_and_delete_user_and_bundled_skills() {
        let temp = tempdir().expect("tempdir");
        let bundled = temp.path().join("bundled");
        let user = temp.path().join("user");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&user).unwrap();

        let source = temp.path().join("my-skill.md");
        std::fs::write(
            &source,
            "# My Skill\n\nAlways quantify achievements without inventing metrics.\n",
        )
        .unwrap();

        let registry =
            FileSkillRegistry::with_paths(bundled.clone(), user.clone()).expect("registry");
        let imported = registry.import_skill(&source).expect("import skill");
        assert_eq!(imported.id, "my-skill");
        assert!(imported.enabled);
        assert!(user.join("my-skill").join("SKILL.md").exists());

        registry
            .set_skill_enabled("my-skill", false)
            .expect("disable skill");
        assert!(!registry.get("my-skill").unwrap().enabled);
        assert!(
            registry
                .compose_system_prompt(&["my-skill".to_string()])
                .unwrap()
                .is_empty()
        );

        // Disabled state survives a restart.
        let restarted =
            FileSkillRegistry::with_paths(bundled.clone(), user.clone()).expect("registry restart");
        assert!(!restarted.get("my-skill").unwrap().enabled);

        restarted
            .set_skill_enabled("my-skill", true)
            .expect("enable skill");
        assert!(restarted.get("my-skill").unwrap().enabled);
        assert!(
            restarted
                .compose_system_prompt(&["my-skill".to_string()])
                .unwrap()
                .contains("quantify achievements")
        );

        restarted
            .delete_skill("my-skill")
            .expect("delete user skill");
        assert!(restarted.get("my-skill").is_none());
        assert!(!user.join("my-skill").exists());

        // Bundled skills can also be deleted and stay deleted across restarts, with restore support.
        std::fs::write(
            bundled.join("builtin.toml"),
            "id = \"builtin\"\nname = \"Builtin\"\n[prompt_injection]\nsystem_directives = \"x\"\n",
        )
        .unwrap();
        let registry = FileSkillRegistry::with_paths(bundled.clone(), user.clone())
            .expect("registry with builtin");
        assert!(registry.get("builtin").is_some());
        registry
            .delete_skill("builtin")
            .expect("delete bundled skill");
        assert!(registry.get("builtin").is_none());
        assert!(registry.has_deleted_bundled_skills());

        // Deletion persists across restarts.
        let restarted = FileSkillRegistry::with_paths(bundled, user).expect("registry restart 2");
        assert!(restarted.get("builtin").is_none());
        assert!(restarted.has_deleted_bundled_skills());

        let restored = restarted
            .restore_bundled_skills()
            .expect("restore defaults");
        assert_eq!(restored, 1);
        assert!(restarted.get("builtin").is_some());
        assert!(!restarted.has_deleted_bundled_skills());
    }

    #[test]
    fn test_reset_to_defaults_removes_user_skills_and_restores_bundled() {
        let temp = tempdir().expect("tempdir");
        let bundled = temp.path().join("bundled");
        let user = temp.path().join("user");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&user).unwrap();

        std::fs::write(
            bundled.join("default_a.toml"),
            "id = \"default_a\"\nname = \"Default A\"\n[prompt_injection]\nsystem_directives = \"A\"\n",
        )
        .unwrap();
        std::fs::write(
            bundled.join("default_b.toml"),
            "id = \"default_b\"\nname = \"Default B\"\n[prompt_injection]\nsystem_directives = \"B\"\n",
        )
        .unwrap();
        std::fs::write(
            user.join("custom_x.toml"),
            "id = \"custom_x\"\nname = \"Custom X\"\n[prompt_injection]\nsystem_directives = \"X\"\n",
        )
        .unwrap();

        let registry = FileSkillRegistry::with_paths(bundled, user.clone()).expect("registry");
        registry.set_skill_enabled("default_a", false).unwrap();
        registry.delete_skill("default_b").unwrap();
        assert_eq!(registry.list().len(), 2);

        registry.reset_to_defaults().expect("reset_to_defaults");

        assert!(registry.get("custom_x").is_none());
        assert!(registry.get("default_a").unwrap().enabled);
        assert!(registry.get("default_b").unwrap().enabled);
        assert!(!registry.has_deleted_bundled_skills());
        assert!(!user.join("custom_x.toml").exists());
    }
}
