use vanguard_cv::contracts::*;
use vanguard_cv::mocks::{MockPrivacy, sample_root_document};

#[test]
fn root_guard_rejects_ai_batch_updates() {
    let repo = vanguard_cv::mocks::InMemoryRepository::with_sample_data();
    let root = repo.list_roots().unwrap().remove(0);
    let doc = repo.load(root.id).unwrap();
    let error = repo.save_ai_batch(&doc).unwrap_err().to_string();
    assert!(error.contains("write-guard"), "unexpected error: {error}");
}

#[test]
fn branching_deep_clones_and_preserves_origin_ids() {
    let repo = vanguard_cv::mocks::InMemoryRepository::with_sample_data();
    let root = repo.list_roots().unwrap().remove(0);
    let root_doc = repo.load(root.id).unwrap();

    let child = repo
        .branch_sub_variation(
            root.id,
            "CrowdStrike - Red Team",
            JobContext {
                company_name: "CrowdStrike".to_string(),
                target_role: "Red Team Operator".to_string(),
                job_description_raw: "Rust, detection engineering, adversary emulation".to_string(),
                ..JobContext::default()
            },
        )
        .unwrap();

    assert_eq!(child.parent_id, Some(root.id));
    assert_eq!(child.body.sections.len(), root_doc.body.sections.len());
    assert_ne!(child.body.sections[0].id, root_doc.body.sections[0].id);
    assert_eq!(
        child.body.sections[0].origin_item_id,
        Some(root_doc.body.sections[0].id)
    );
    assert_eq!(repo.list_children(root.id).unwrap().len(), 1);
}

#[test]
fn privacy_scrubber_removes_pii_from_outbound_json() {
    let doc = sample_root_document();
    let mut body = doc.body.clone();
    body.sections.push(CvSection {
        id: uuid::Uuid::new_v4(),
        origin_item_id: None,
        title: "SUMMARY".to_string(),
        enabled: true,
        kind: SectionKind::Summary {
            text: format!(
                "{} can be reached at {} or {}.",
                doc.pii_header.full_name,
                doc.pii_header.email.clone().unwrap(),
                doc.pii_header.phone.clone().unwrap()
            ),
        },
    });

    let scrubber = MockPrivacy;
    let payload = scrubber.scrub(&body, &doc.pii_header).unwrap();
    assert!(payload.redaction_count >= 3);
    assert!(!payload.json.contains("Alex Morgan"));
    assert!(!payload.json.contains("alex.morgan@example.com"));
    assert!(!payload.json.contains("555 010 4242"));
    assert!(payload.json.contains("[REDACTED_PII_"));
}

#[test]
fn mock_pdf_renders_a4_pages() {
    let doc = sample_root_document();
    let pages = vanguard_cv::mocks::MockPdfCompiler
        .render_preview_pages(&doc.pii_header, &doc.body, 1)
        .unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].rgba.len(), 794 * 1123 * 4);
}

#[tokio::test]
async fn mock_ai_produces_reviewable_proposals() {
    use vanguard_cv::mocks::MockAiOrchestrator;

    let doc = sample_root_document();
    let payload = MockPrivacy.scrub(&doc.body, &doc.pii_header).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = MockAiOrchestrator::default()
        .generate_variation(
            VariationRequest {
                child_cv_id: uuid::Uuid::new_v4(),
                job_context: JobContext {
                    company_name: "CrowdStrike".to_string(),
                    target_role: "Red Team Operator".to_string(),
                    ..JobContext::default()
                },
                skill_ids: vec!["xyz_bullet_enhancer".to_string()],
            },
            payload,
            tx,
        )
        .await
        .unwrap();

    assert!(!outcome.proposals.is_empty());
    assert!(outcome.ats_score.is_some());
    let mut event_count = 0;
    while rx.try_recv().is_ok() {
        event_count += 1;
    }
    assert!(event_count >= 3);
}
