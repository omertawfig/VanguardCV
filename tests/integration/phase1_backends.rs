//! Phase 2 integration gate.
//!
//! These tests exercise the real SQLite repository, the real Aho-Corasick PII
//! scrubber, and the real embedded Typst compiler together — no mocks.

use vanguard_cv::contracts::{
    BulletItem, CvRepository, CvSection, JobContext, PdfCompiler, PrivacyGuard, SectionKind,
};
use vanguard_cv::db::lineage_diff::{ChangeKind, DiffSpanKind, compute_diff};
use vanguard_cv::db::repository::SqliteRepository;
use vanguard_cv::db::snapshots::snapshot_before_ai;
use vanguard_cv::mocks::sample_root_document;
use vanguard_cv::pdf::exporter::{export_filename, write_pdf};
use vanguard_cv::pdf::typst_compiler::TypstCompiler;
use vanguard_cv::privacy::scrubber::PiiScrubber;

fn job_context() -> JobContext {
    JobContext {
        company_name: "CrowdStrike".to_string(),
        target_role: "Red Team Operator".to_string(),
        job_description_raw: "Operate adversary emulation engagements.".to_string(),
        application_url: None,
        applied_skill_ids: vec!["ats_classic_optimizer".to_string()],
        ats_match_score: None,
    }
}

#[test]
fn sqlite_seed_branch_and_lineage_round_trip() -> anyhow::Result<()> {
    let repo = SqliteRepository::in_memory()?;

    assert!(repo.seed_if_empty()?, "first seed should insert");
    assert!(!repo.seed_if_empty()?, "second seed must be a no-op");

    let roots = repo.list_roots()?;
    assert_eq!(roots.len(), 1);
    let root = repo.load(roots[0].id)?;

    let titles: Vec<&str> = root
        .body
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert!(titles.contains(&"PROFESSIONAL EXPERIENCE"));
    assert!(titles.contains(&"EDUCATION"));
    assert!(titles.contains(&"SKILLS AND OTHERS"));
    assert!(titles.contains(&"HONORS AND AWARDS:"));
    assert!(titles.contains(&"TRAINING COURSES:"));

    let child = repo.branch_sub_variation(root.id, "CrowdStrike — Red Team", job_context())?;
    assert_eq!(child.parent_id, Some(root.id));
    assert_eq!(child.parent_version_snapshot, root.version);
    assert_eq!(child.body.sections.len(), root.body.sections.len());

    // Every cloned section is a fresh id, with origin pointing at the root.
    for (child_section, root_section) in child.body.sections.iter().zip(&root.body.sections) {
        assert_ne!(child_section.id, root_section.id);
        assert_eq!(child_section.origin_item_id, Some(root_section.id));
    }

    // Children are listed under the root and are writable.
    let children = repo.list_children(root.id)?;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, child.id);

    let mut edited = child.clone();
    let version_before = edited.version;
    edited.title = "CrowdStrike — Red Team (edited)".to_string();
    repo.save(&edited)?;
    edited = repo.load(edited.id)?;
    assert!(edited.version > version_before);
    assert_eq!(edited.parent_id, Some(root.id));

    // Root write-guard: AI batches may never target a root.
    let mut root_edit = root.clone();
    root_edit.body.sections.clear();
    let guard = repo.save_ai_batch(&root_edit);
    assert!(guard.is_err(), "root AI batch must be rejected");

    // Snapshots restore content and bump the version.
    let snapshot = repo.create_snapshot(edited.id, "before experiment")?;
    let mut changed = edited.clone();
    changed.body.sections.clear();
    repo.save(&changed)?;
    let restored = repo.restore_snapshot(snapshot.id)?;
    assert!(!restored.body.sections.is_empty());
    assert!(restored.version > version_before);

    Ok(())
}

#[test]
fn real_scrubber_enforces_the_pii_air_gap() -> anyhow::Result<()> {
    let mut doc = sample_root_document();
    let pii = doc.pii_header.clone();
    let name = pii.full_name.clone();
    let email = pii.email.clone().expect("sample email");
    let phone = pii.phone.clone().expect("sample phone");

    doc.body.sections.push(CvSection {
        id: uuid::Uuid::new_v4(),
        origin_item_id: None,
        title: "SUMMARY".to_string(),
        enabled: true,
        kind: SectionKind::Summary {
            text: format!("{name} can be reached at {email} or {phone}."),
        },
    });
    doc.body.sections.push(CvSection {
        id: uuid::Uuid::new_v4(),
        origin_item_id: None,
        title: "HONORS AND AWARDS:".to_string(),
        enabled: true,
        kind: SectionKind::SimpleBulletList {
            items: vec![BulletItem::new(format!(
                "{name} reported a vulnerability; follow up with {email}."
            ))],
        },
    });

    let scrubber = PiiScrubber;
    let scrubbed = scrubber.scrub(&doc.body, &doc.pii_header)?;

    assert!(scrubbed.redaction_count >= 4);
    assert!(scrubbed.json.contains("[REDACTED_PII_"));
    for secret in [name.as_str(), email.as_str(), phone.as_str()] {
        assert!(
            !scrubbed.json.contains(secret),
            "outbound JSON leaked {secret:?}: {}",
            scrubbed.json
        );
    }
    // The username prefix used in the email must not survive either.
    let username = email.split('@').next().unwrap_or_default();
    assert!(!scrubbed.json.contains(username));

    // Token is bound to the exact audited payload.
    let inspected = scrubber.inspect_outbound_payload(&doc.body, &doc.pii_header)?;
    assert_eq!(inspected, scrubbed.json);
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for byte in scrubbed.json.as_bytes() {
        digest ^= u64::from(*byte);
        digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(scrubbed.token.payload_digest(), digest);

    // The original body must not be mutated.
    assert!(matches!(
        doc.body.sections.last().map(|section| &section.kind),
        Some(SectionKind::SimpleBulletList { .. })
    ));
    let serialized = serde_json::to_string(&doc.body)?;
    assert!(serialized.contains(&name));

    Ok(())
}

#[test]
fn real_typst_engine_exports_and_renders() -> anyhow::Result<()> {
    let doc = sample_root_document();
    let compiler = TypstCompiler::new()?;

    let pdf = compiler.compile_to_pdf(&doc.pii_header, &doc.body)?;
    assert!(pdf.starts_with(b"%PDF"));
    assert!(pdf.len() > 2_000);

    let pages = compiler.render_preview_pages(&doc.pii_header, &doc.body, 3)?;
    assert!(!pages.is_empty());
    assert_eq!(pages[0].page_index, 0);
    assert!(pages[0].width_px > 1_000);
    assert_eq!(
        pages[0].rgba.len(),
        (pages[0].width_px * pages[0].height_px * 4) as usize
    );

    // A4 aspect ratio, within rounding.
    let ratio = f64::from(pages[0].width_px) / f64::from(pages[0].height_px);
    assert!((ratio - 0.707).abs() < 0.01, "unexpected page ratio {ratio}");

    let export_dir = tempfile::tempdir()?;
    let path = write_pdf(&doc, &pdf, export_dir.path())?;
    assert!(path.exists());
    assert_eq!(path.file_name().unwrap().to_string_lossy(), export_filename(&doc));
    assert_eq!(std::fs::read(path)?, pdf);

    Ok(())
}

#[test]
fn imported_json_detaches_lineage() -> anyhow::Result<()> {
    let repo = SqliteRepository::in_memory()?;
    repo.seed_if_empty()?;
    let root = repo.load(repo.list_roots()?[0].id)?;
    let child = repo.branch_sub_variation(root.id, "Variation", job_context())?;

    let json = repo.export_json(child.id)?;
    let imported = repo.import_json(&json)?;

    assert_ne!(imported.id, child.id);
    assert_eq!(imported.parent_id, None);
    assert_eq!(imported.title, child.title);
    assert_eq!(imported.body.sections.len(), child.body.sections.len());

    Ok(())
}

#[test]
fn lineage_diff_detects_child_edits_against_the_real_database() -> anyhow::Result<()> {
    let repo = SqliteRepository::in_memory()?;
    repo.seed_if_empty()?;
    let root = repo.load(repo.list_roots()?[0].id)?;
    let child = repo.branch_sub_variation(root.id, "Tailored Variation", job_context())?;

    // An untouched clone has no changes.
    let unchanged = compute_diff(&child, &root)?;
    assert!(!unchanged.has_changes());

    // Edit one cloned bullet and save it through SQLite.
    let mut edited = child.clone();
    let mut changed_bullet_id = None;
    for section in &mut edited.body.sections {
        if let SectionKind::TimelineEntries { entries } = &mut section.kind
            && let Some(entry) = entries.first_mut()
            && let Some(bullet) = entry.bullets.first_mut()
        {
            bullet.text = format!("{} with Kubernetes exposure.", bullet.text);
            changed_bullet_id = Some(bullet.id);
            break;
        }
    }
    let changed_bullet_id = changed_bullet_id.expect("seed has a timeline bullet");
    let expected_origin = child
        .body
        .bullets()
        .iter()
        .find(|bullet| bullet.id == changed_bullet_id)
        .and_then(|bullet| bullet.origin_item_id);

    repo.save(&edited)?;
    let reloaded_child = repo.load(edited.id)?;
    let reloaded_root = repo.load(root.id)?;

    let diff = compute_diff(&reloaded_child, &reloaded_root)?;
    assert!(diff.has_changes());
    assert!(diff.changed_item_ids.contains(&changed_bullet_id));

    let bullet_diff = diff
        .item_diffs
        .iter()
        .find(|item| item.item_id == changed_bullet_id)
        .expect("edited bullet diff");
    assert_eq!(bullet_diff.change_kind, ChangeKind::Modified);
    assert_eq!(bullet_diff.origin_item_id, expected_origin);
    assert!(
        bullet_diff
            .text_diff
            .iter()
            .any(|span| span.kind == DiffSpanKind::Added && span.text.contains("Kubernetes"))
    );

    // The parent is untouched by the child edit.
    assert_eq!(reloaded_root.version, root.version);

    // Pre-AI snapshot helper works against the real repo.
    let snapshot = snapshot_before_ai(&repo, reloaded_child.id, "before AI pass")?;
    assert_eq!(snapshot.cv_id, reloaded_child.id);

    Ok(())
}
