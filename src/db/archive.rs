//! Portable `.vanguard` workspace archives and Zero-Trust anonymized JSON templates.
//!
//! - A `.vanguard` archive bundles every Master CV, every Child Sub-Variation
//!   (preserving full parent-child `parent_id` and `origin_item_id` lineage),
//!   and user-authored custom skills (`SKILL.md` / `.toml`).
//! - An anonymized JSON template runs the document body through [`PrivacyGuard`]
//!   and replaces the local [`PiiContactHeader`] with clean placeholders so a
//!   user can share CV structure and wording safely without leaking PII.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::contracts::{
    CvDocument, CvRepository, DocumentId, ItemId, PiiContactHeader, PrivacyGuard, SanitizedCvBody,
    SectionKind, SkillRegistry,
};

pub const VANGUARD_ARCHIVE_FORMAT: &str = "vanguardcv.archive";
pub const VANGUARD_ARCHIVE_SCHEMA_VERSION: u32 = 1;

fn default_true() -> bool {
    true
}

/// A user-authored skill bundled inside a `.vanguard` archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedSkill {
    pub id: String,
    pub filename: String,
    pub content: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Self-contained portable workspace archive (`.vanguard`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VanguardArchive {
    pub format: String,
    pub schema_version: u32,
    pub exported_at: DateTime<Utc>,
    pub app_version: String,
    pub documents: Vec<CvDocument>,
    #[serde(default)]
    pub user_skills: Vec<ArchivedSkill>,
}

/// Summary returned after importing a `.vanguard` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveImportOutcome {
    pub imported_masters: usize,
    pub imported_variations: usize,
    pub imported_skills: usize,
    pub first_doc_id: Option<DocumentId>,
}

/// Exports the entire workspace (all masters, variations, and user custom skills)
/// into a portable `.vanguard` archive payload.
pub fn export_vanguard_archive(
    repo: &dyn CvRepository,
    skills: &dyn SkillRegistry,
) -> Result<String> {
    let mut documents = Vec::new();
    for root_summary in repo.list_roots()? {
        let root_doc = repo.load(root_summary.id)?;
        documents.push(root_doc);
        for child_summary in repo.list_children(root_summary.id)? {
            let child_doc = repo.load(child_summary.id)?;
            documents.push(child_doc);
        }
    }

    let user_dir = skills.skills_dir();
    let canonical_user_dir = std::fs::canonicalize(&user_dir).unwrap_or_else(|_| user_dir.clone());
    let mut user_skills = Vec::new();

    for manifest in skills.list() {
        let Some(src_path) = manifest.source_path.as_ref() else {
            continue;
        };
        let canonical_src = std::fs::canonicalize(src_path).unwrap_or_else(|_| src_path.clone());
        if !canonical_src.starts_with(&canonical_user_dir) && !src_path.starts_with(&user_dir) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(src_path) {
            let filename = src_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("SKILL.md")
                .to_string();
            user_skills.push(ArchivedSkill {
                id: manifest.id,
                filename,
                content,
                enabled: manifest.enabled,
            });
        }
    }

    let archive = VanguardArchive {
        format: VANGUARD_ARCHIVE_FORMAT.to_string(),
        schema_version: VANGUARD_ARCHIVE_SCHEMA_VERSION,
        exported_at: Utc::now(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        documents,
        user_skills,
    };

    serde_json::to_string_pretty(&archive).context("serializing .vanguard archive")
}

/// Imports a `.vanguard` archive into `repo` and `skills`, assigning fresh IDs
/// while preserving master-to-variation parent links and `origin_item_id` lineage.
pub fn import_vanguard_archive(
    raw: &str,
    repo: &dyn CvRepository,
    skills: &dyn SkillRegistry,
) -> Result<ArchiveImportOutcome> {
    let archive: VanguardArchive =
        serde_json::from_str(raw).context("parsing .vanguard workspace archive")?;

    if archive.format != VANGUARD_ARCHIVE_FORMAT {
        bail!(
            "unsupported archive format {:?} (expected {:?})",
            archive.format,
            VANGUARD_ARCHIVE_FORMAT
        );
    }
    if archive.schema_version != VANGUARD_ARCHIVE_SCHEMA_VERSION {
        bail!(
            "unsupported .vanguard schema version {} (supported: {})",
            archive.schema_version,
            VANGUARD_ARCHIVE_SCHEMA_VERSION
        );
    }

    let (roots, children): (Vec<CvDocument>, Vec<CvDocument>) = archive
        .documents
        .into_iter()
        .partition(|doc| doc.parent_id.is_none());

    let mut root_id_map: HashMap<DocumentId, DocumentId> = HashMap::new();
    let mut root_item_map: HashMap<(DocumentId, ItemId), ItemId> = HashMap::new();
    let mut imported_masters = 0usize;
    let mut imported_variations = 0usize;
    let mut first_doc_id = None;

    // 1. Import all Root Master CVs first and record old -> new item ID mappings.
    for root in roots {
        let old_root_id = root.id;
        let fresh_pii = rekey_pii_header(&root.pii_header);
        let (fresh_body, item_map) = rekey_root_body(&root.body);
        let created = repo
            .create_root(&root.title, fresh_pii, fresh_body)
            .with_context(|| format!("importing master CV {:?}", root.title))?;

        for (old_item_id, new_item_id) in item_map {
            root_item_map.insert((old_root_id, old_item_id), new_item_id);
        }
        root_id_map.insert(old_root_id, created.id);
        if first_doc_id.is_none() {
            first_doc_id = Some(created.id);
        }
        imported_masters += 1;
    }

    // 2. Import Child Variations linked to their newly created Masters.
    for child in children {
        let old_parent_id = child.parent_id;
        if let Some(old_pid) = old_parent_id
            && let Some(&new_parent_id) = root_id_map.get(&old_pid)
        {
            let job_ctx = child.job_context.clone().unwrap_or_default();
            let mut branched = repo
                .branch_sub_variation(new_parent_id, &child.title, job_ctx.clone())
                .with_context(|| format!("branching imported variation {:?}", child.title))?;

            branched.pii_header = rekey_pii_header(&child.pii_header);
            branched.body = rekey_child_body(&child.body, old_pid, &root_item_map);
            branched.job_context = child.job_context.or(Some(job_ctx));
            repo.save(&branched)
                .with_context(|| format!("saving imported variation {:?}", child.title))?;

            if first_doc_id.is_none() {
                first_doc_id = Some(branched.id);
            }
            imported_variations += 1;
        } else {
            // Orphan variation whose parent was not present in the archive: import as a root.
            let fresh_pii = rekey_pii_header(&child.pii_header);
            let (fresh_body, _) = rekey_root_body(&child.body);
            let created = repo
                .create_root(&child.title, fresh_pii, fresh_body)
                .with_context(|| format!("importing standalone CV {:?}", child.title))?;
            if first_doc_id.is_none() {
                first_doc_id = Some(created.id);
            }
            imported_masters += 1;
        }
    }

    // 3. Restore bundled user skills into the user skills directory.
    let mut imported_skills = 0usize;
    if !archive.user_skills.is_empty() {
        let user_dir = skills.skills_dir();
        std::fs::create_dir_all(&user_dir)
            .with_context(|| format!("creating user skills directory {}", user_dir.display()))?;

        let mut restored_flags: Vec<(String, bool)> = Vec::new();
        for skill in &archive.user_skills {
            let slug = sanitize_skill_slug(&skill.id);
            if slug.is_empty() {
                continue;
            }
            let is_toml = Path::new(&skill.filename)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"));

            let dest_path = if is_toml {
                user_dir.join(format!("{slug}.toml"))
            } else {
                let skill_folder = user_dir.join(&slug);
                std::fs::create_dir_all(&skill_folder).with_context(|| {
                    format!("creating skill directory {}", skill_folder.display())
                })?;
                skill_folder.join("SKILL.md")
            };

            std::fs::write(&dest_path, &skill.content)
                .with_context(|| format!("writing archived skill to {}", dest_path.display()))?;
            restored_flags.push((skill.id.clone(), skill.enabled));
            imported_skills += 1;
        }

        if imported_skills > 0 {
            let _ = skills.reload();
            for (skill_id, enabled) in restored_flags {
                let _ = skills.set_skill_enabled(&skill_id, enabled);
            }
        }
    }

    Ok(ArchiveImportOutcome {
        imported_masters,
        imported_variations,
        imported_skills,
        first_doc_id,
    })
}

/// Exports a single CV as a Zero-Trust anonymized JSON template.
///
/// All contact PII is removed from `pii_header`, and the CV body is scrubbed
/// through [`PrivacyGuard::scrub`] so no personal details leak anywhere in the
/// exported JSON. The resulting JSON can be imported directly via
/// [`CvRepository::import_json`].
pub fn export_anonymized_template_json(
    doc: &CvDocument,
    privacy: &dyn PrivacyGuard,
) -> Result<String> {
    let scrubbed = privacy
        .scrub(&doc.body, &doc.pii_header)
        .context("scrubbing CV body for anonymized template export")?;

    let mut clean_body = scrubbed.body;
    clear_origin_links(&mut clean_body);

    let trimmed_title = doc.title.trim();
    let template_title = if trimmed_title.is_empty() {
        "CV Template".to_string()
    } else if trimmed_title.to_ascii_lowercase().contains("template") {
        trimmed_title.to_string()
    } else {
        format!("{trimmed_title} (Template)")
    };

    let now = Utc::now();
    let template_doc = CvDocument {
        id: Uuid::new_v4(),
        parent_id: None,
        parent_version_snapshot: 1,
        title: template_title,
        version: 1,
        created_at: now,
        updated_at: now,
        job_context: None,
        pii_header: PiiContactHeader {
            full_name: "Your Name".to_string(),
            headline_job_title: doc.pii_header.headline_job_title.clone(),
            email: None,
            phone: None,
            location: None,
            social_links: Vec::new(),
            custom_fields: Vec::new(),
        },
        body: clean_body,
    };

    serde_json::to_string_pretty(&template_doc).context("serializing anonymized template JSON")
}

fn rekey_pii_header(header: &PiiContactHeader) -> PiiContactHeader {
    let mut cloned = header.clone();
    for link in &mut cloned.social_links {
        link.id = Uuid::new_v4();
    }
    for field in &mut cloned.custom_fields {
        field.id = Uuid::new_v4();
    }
    cloned
}

fn rekey_root_body(body: &SanitizedCvBody) -> (SanitizedCvBody, HashMap<ItemId, ItemId>) {
    let mut cloned = body.clone();
    let mut map = HashMap::new();

    for sec in &mut cloned.sections {
        let new_sec_id = Uuid::new_v4();
        map.insert(sec.id, new_sec_id);
        sec.id = new_sec_id;
        sec.origin_item_id = None;

        match &mut sec.kind {
            SectionKind::Summary { .. } => {}
            SectionKind::TimelineEntries { entries } => {
                for entry in entries {
                    let new_entry_id = Uuid::new_v4();
                    map.insert(entry.id, new_entry_id);
                    entry.id = new_entry_id;
                    entry.origin_item_id = None;

                    for bullet in &mut entry.bullets {
                        let new_bullet_id = Uuid::new_v4();
                        map.insert(bullet.id, new_bullet_id);
                        bullet.id = new_bullet_id;
                        bullet.origin_item_id = None;
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for cat in categories {
                    let new_cat_id = Uuid::new_v4();
                    map.insert(cat.id, new_cat_id);
                    cat.id = new_cat_id;
                    cat.origin_item_id = None;
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for bullet in items {
                    let new_bullet_id = Uuid::new_v4();
                    map.insert(bullet.id, new_bullet_id);
                    bullet.id = new_bullet_id;
                    bullet.origin_item_id = None;
                }
            }
        }
    }

    (cloned, map)
}

fn rekey_child_body(
    body: &SanitizedCvBody,
    old_parent_id: DocumentId,
    root_item_map: &HashMap<(DocumentId, ItemId), ItemId>,
) -> SanitizedCvBody {
    let mut cloned = body.clone();
    let remap = |origin: Option<ItemId>| -> Option<ItemId> {
        origin.and_then(|old_id| root_item_map.get(&(old_parent_id, old_id)).copied())
    };

    for sec in &mut cloned.sections {
        sec.id = Uuid::new_v4();
        sec.origin_item_id = remap(sec.origin_item_id);

        match &mut sec.kind {
            SectionKind::Summary { .. } => {}
            SectionKind::TimelineEntries { entries } => {
                for entry in entries {
                    entry.id = Uuid::new_v4();
                    entry.origin_item_id = remap(entry.origin_item_id);

                    for bullet in &mut entry.bullets {
                        bullet.id = Uuid::new_v4();
                        bullet.origin_item_id = remap(bullet.origin_item_id);
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for cat in categories {
                    cat.id = Uuid::new_v4();
                    cat.origin_item_id = remap(cat.origin_item_id);
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for bullet in items {
                    bullet.id = Uuid::new_v4();
                    bullet.origin_item_id = remap(bullet.origin_item_id);
                }
            }
        }
    }

    cloned
}

fn clear_origin_links(body: &mut SanitizedCvBody) {
    for sec in &mut body.sections {
        sec.id = Uuid::new_v4();
        sec.origin_item_id = None;
        match &mut sec.kind {
            SectionKind::Summary { .. } => {}
            SectionKind::TimelineEntries { entries } => {
                for entry in entries {
                    entry.id = Uuid::new_v4();
                    entry.origin_item_id = None;
                    for bullet in &mut entry.bullets {
                        bullet.id = Uuid::new_v4();
                        bullet.origin_item_id = None;
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for cat in categories {
                    cat.id = Uuid::new_v4();
                    cat.origin_item_id = None;
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for bullet in items {
                    bullet.id = Uuid::new_v4();
                    bullet.origin_item_id = None;
                }
            }
        }
    }
}

fn sanitize_skill_slug(raw: &str) -> String {
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if (ch == '_' || ch == '-' || ch == ' ') && !slug.is_empty() && !prev_sep {
            slug.push('_');
            prev_sep = true;
        }
    }
    slug.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::skill_registry::FileSkillRegistry;
    use crate::contracts::{BulletItem, JobContext};
    use crate::db::repository::SqliteRepository;
    use crate::mocks::sample_root_document;
    use crate::privacy::scrubber::PiiScrubber;
    use tempfile::TempDir;

    #[test]
    fn vanguard_archive_roundtrip_preserves_lineage_and_user_skills() {
        let src_repo = SqliteRepository::in_memory().unwrap();
        let sample = sample_root_document();
        let master = src_repo
            .create_root(
                &sample.title,
                sample.pii_header.clone(),
                sample.body.clone(),
            )
            .unwrap();

        let child_ctx = JobContext {
            company_name: "CrowdStrike".into(),
            target_role: "Senior Detection Engineer".into(),
            job_description_raw: "Build Rust telemetry pipelines".into(),
            application_url: None,
            applied_skill_ids: vec!["xyz_bullet_enhancer".into()],
            ats_match_score: Some(88),
        };
        let mut child = src_repo
            .branch_sub_variation(master.id, "CrowdStrike — Detection", child_ctx)
            .unwrap();
        // Add a child-only bullet to verify child edits survive roundtrip.
        if let Some(sec) = child
            .body
            .sections
            .iter_mut()
            .find(|s| matches!(s.kind, SectionKind::TimelineEntries { .. }))
            && let SectionKind::TimelineEntries { entries } = &mut sec.kind
        {
            entries[0]
                .bullets
                .push(BulletItem::new("Built custom eBPF sensor prototype."));
        }
        src_repo.save(&child).unwrap();

        let src_bundled = TempDir::new().unwrap();
        let src_user = TempDir::new().unwrap();
        let custom_skill_dir = src_user.path().join("red_team_focus");
        std::fs::create_dir_all(&custom_skill_dir).unwrap();
        std::fs::write(
            custom_skill_dir.join("SKILL.md"),
            "---\nid: red_team_focus\nname: Red Team Focus\ncategory: tailoring\ndescription: Highlights offensive tradecraft\n---\n\nEmphasize exploit chains and detection evasions.\n",
        )
        .unwrap();
        let src_skills = FileSkillRegistry::with_paths(
            src_bundled.path().to_path_buf(),
            src_user.path().to_path_buf(),
        )
        .unwrap();

        let exported = export_vanguard_archive(&src_repo, &src_skills).unwrap();

        // Import into a brand new empty workspace & skill registry
        let dst_repo = SqliteRepository::in_memory().unwrap();
        let dst_bundled = TempDir::new().unwrap();
        let dst_user = TempDir::new().unwrap();
        let dst_skills = FileSkillRegistry::with_paths(
            dst_bundled.path().to_path_buf(),
            dst_user.path().to_path_buf(),
        )
        .unwrap();

        let outcome = import_vanguard_archive(&exported, &dst_repo, &dst_skills).unwrap();
        assert_eq!(outcome.imported_masters, 1);
        assert_eq!(outcome.imported_variations, 1);
        assert_eq!(outcome.imported_skills, 1);
        assert!(outcome.first_doc_id.is_some());

        let dst_roots = dst_repo.list_roots().unwrap();
        assert_eq!(dst_roots.len(), 1);
        let imported_master = dst_repo.load(dst_roots[0].id).unwrap();
        assert_ne!(imported_master.id, master.id);

        let dst_children = dst_repo.list_children(imported_master.id).unwrap();
        assert_eq!(dst_children.len(), 1);
        let imported_child = dst_repo.load(dst_children[0].id).unwrap();
        assert_eq!(imported_child.parent_id, Some(imported_master.id));
        assert_eq!(
            imported_child.job_context.as_ref().unwrap().company_name,
            "CrowdStrike"
        );
        // Verify origin_item_id links point to the newly imported master's section IDs.
        assert_eq!(
            imported_child.body.sections[0].origin_item_id,
            Some(imported_master.body.sections[0].id)
        );
        // Verify child-specific bullet was preserved.
        let has_ebpf = imported_child
            .body
            .bullets()
            .iter()
            .any(|b| b.text.contains("eBPF sensor prototype"));
        assert!(has_ebpf);

        // Verify custom skill was restored.
        let restored_skill = dst_skills
            .get("red_team_focus")
            .expect("custom skill restored");
        assert_eq!(restored_skill.name, "Red Team Focus");
    }

    #[test]
    fn vanguard_archive_rejects_invalid_format_or_schema() {
        let repo = SqliteRepository::in_memory().unwrap();
        let bundled = TempDir::new().unwrap();
        let user = TempDir::new().unwrap();
        let skills =
            FileSkillRegistry::with_paths(bundled.path().to_path_buf(), user.path().to_path_buf())
                .unwrap();

        let bad_format = r#"{"format":"other","schema_version":1,"exported_at":"2026-01-01T00:00:00Z","app_version":"0.1.0","documents":[]}"#;
        assert!(import_vanguard_archive(bad_format, &repo, &skills).is_err());

        let bad_version = r#"{"format":"vanguardcv.archive","schema_version":99,"exported_at":"2026-01-01T00:00:00Z","app_version":"0.1.0","documents":[]}"#;
        assert!(import_vanguard_archive(bad_version, &repo, &skills).is_err());
    }

    #[test]
    fn anonymized_template_strips_all_pii_and_roundtrips_import() {
        let mut doc = sample_root_document();
        // Inject stray PII into a bullet to prove PrivacyGuard scrubs body text too.
        let email = doc.pii_header.email.clone().unwrap();
        let full_name = doc.pii_header.full_name.clone();
        if let Some(sec) = doc
            .body
            .sections
            .iter_mut()
            .find(|s| matches!(s.kind, SectionKind::Summary { .. }))
            && let SectionKind::Summary { text } = &mut sec.kind
        {
            text.push_str(&format!(" Contact {full_name} at {email}."));
        }

        let privacy = PiiScrubber;
        let json = export_anonymized_template_json(&doc, &privacy).unwrap();

        assert!(!json.contains(&full_name));
        assert!(!json.contains(&email));
        if let Some(phone) = &doc.pii_header.phone {
            assert!(!json.contains(phone));
        }

        // Must import cleanly via standard CvRepository::import_json
        let repo = SqliteRepository::in_memory().unwrap();
        let imported = repo.import_json(&json).unwrap();
        assert_eq!(imported.pii_header.full_name, "Your Name");
        assert!(imported.pii_header.email.is_none());
        assert!(imported.pii_header.social_links.is_empty());
        assert!(imported.pii_header.custom_fields.is_empty());
        assert!(imported.title.contains("Template"));
    }
}
