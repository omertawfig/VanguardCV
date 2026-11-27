use super::*;

#[cfg(test)]
mod drag_reorder_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn reorders_sections_entries_bullets_and_skill_categories() {
        let mut doc = crate::mocks::sample_root_document();
        let orig_sec0 = doc.body.sections[0].id;
        let orig_sec1 = doc.body.sections[1].id;
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::Section { section_id: orig_sec0 },
            &EditorDragKind::Section { section_id: orig_sec1 },
        ));
        assert_eq!(doc.body.sections[0].id, orig_sec1);
        assert_eq!(doc.body.sections[1].id, orig_sec0);

        // Reorder timeline entries & nested bullets
        let exp_sec = doc
            .body
            .sections
            .iter()
            .find(|s| matches!(s.kind, SectionKind::TimelineEntries { ref entries } if entries.len() >= 2))
            .expect("sample has experience timeline entries");
        let exp_sec_id = exp_sec.id;
        let (e0, e1, b0, b1) = match &exp_sec.kind {
            SectionKind::TimelineEntries { entries } => {
                (entries[0].id, entries[1].id, entries[0].bullets[0].id, entries[0].bullets[1].id)
            }
            _ => unreachable!(),
        };
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::TimelineEntry {
                section_id: exp_sec_id,
                entry_id: e0,
            },
            &EditorDragKind::TimelineEntry {
                section_id: exp_sec_id,
                entry_id: e1,
            },
        ));
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::Bullet {
                section_id: exp_sec_id,
                entry_id: Some(e0),
                bullet_id: b0,
            },
            &EditorDragKind::Bullet {
                section_id: exp_sec_id,
                entry_id: Some(e0),
                bullet_id: b1,
            },
        ));
        let exp_after = doc.body.sections.iter().find(|s| s.id == exp_sec_id).unwrap();
        if let SectionKind::TimelineEntries { entries } = &exp_after.kind {
            assert_eq!(entries[0].id, e1);
            assert_eq!(entries[1].id, e0);
            assert_eq!(entries[1].bullets[0].id, b1);
            assert_eq!(entries[1].bullets[1].id, b0);
        }

        // Reorder skill categories
        let skill_sec = doc
            .body
            .sections
            .iter()
            .find(|s| matches!(s.kind, SectionKind::CategorizedInlineList { ref categories } if categories.len() >= 2))
            .expect("sample has skill categories");
        let skill_sec_id = skill_sec.id;
        let (c0, c1) = match &skill_sec.kind {
            SectionKind::CategorizedInlineList { categories } => (categories[0].id, categories[1].id),
            _ => unreachable!(),
        };
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::SkillCategory {
                section_id: skill_sec_id,
                category_id: c0,
            },
            &EditorDragKind::SkillCategory {
                section_id: skill_sec_id,
                category_id: c1,
            },
        ));
        let skill_after = doc.body.sections.iter().find(|s| s.id == skill_sec_id).unwrap();
        if let SectionKind::CategorizedInlineList { categories } = &skill_after.kind {
            assert_eq!(categories[0].id, c1);
            assert_eq!(categories[1].id, c0);
        }

        // Reorder social links and custom header fields on Personal Info
        while doc.pii_header.social_links.len() < 2 {
            doc.pii_header.social_links.push(SocialLink {
                id: Uuid::new_v4(),
                platform: "Portfolio".into(),
                display_text: "example.dev".into(),
                url: "https://example.dev".into(),
                enabled: true,
            });
        }
        assert!(doc.pii_header.social_links.len() >= 2);
        let l0 = doc.pii_header.social_links[0].id;
        let l1 = doc.pii_header.social_links[1].id;
        assert!(reorder_item_in_doc(
            &mut doc,
            &EditorDragKind::SocialLink { link_id: l0 },
            &EditorDragKind::SocialLink { link_id: l1 },
        ));
        assert_eq!(doc.pii_header.social_links[0].id, l1);
        assert_eq!(doc.pii_header.social_links[1].id, l0);

        while doc.pii_header.custom_fields.len() < 2 {
            doc.pii_header.custom_fields.push(CustomHeaderField {
                id: Uuid::new_v4(),
                label: "Clearance".into(),
                value: "EU Secret".into(),
                enabled: true,
            });
        }
        assert!(doc.pii_header.custom_fields.len() >= 2);
        let f0 = doc.pii_header.custom_fields[0].id;
        let f1 = doc.pii_header.custom_fields[1].id;
        assert!(reorder_item_in_doc(
            &mut doc,
            &EditorDragKind::CustomHeaderField { field_id: f0 },
            &EditorDragKind::CustomHeaderField { field_id: f1 },
        ));
        assert_eq!(doc.pii_header.custom_fields[0].id, f1);
        assert_eq!(doc.pii_header.custom_fields[1].id, f0);
    }

    #[::core::prelude::v1::test]
    fn toggle_visibility_proposal_hides_sections_entries_bullets_and_skill_categories() {
        let mut doc = crate::mocks::sample_root_document();
        let last_sec_id = doc.body.sections.last().expect("section").id;
        let (entry_id, bullet_id) = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::TimelineEntries { entries } => entries.first().and_then(|e| {
                    e.bullets.first().map(|b| (e.id, b.id))
                }),
                _ => None,
            })
            .expect("timeline entry and bullet");
        let cat_id = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::CategorizedInlineList { categories } => {
                    categories.first().map(|c| c.id)
                }
                _ => None,
            })
            .expect("skill category");

        for target in [last_sec_id, entry_id, bullet_id, cat_id] {
            apply_ai_proposal(
                &mut doc,
                &AiProposal::ToggleVisibility {
                    item_id: target,
                    enabled: false,
                    reason: "Irrelevant to target JD".to_string(),
                },
            );
        }

        assert!(
            !doc.body
                .sections
                .iter()
                .find(|s| s.id == last_sec_id)
                .unwrap()
                .enabled
        );
        assert!(!doc.body.find_bullet(bullet_id).unwrap().enabled);
        let entry_enabled = doc.body.sections.iter().find_map(|s| match &s.kind {
            SectionKind::TimelineEntries { entries } => {
                entries.iter().find(|e| e.id == entry_id).map(|e| e.enabled)
            }
            _ => None,
        });
        assert_eq!(entry_enabled, Some(false));
        let cat_enabled = doc.body.sections.iter().find_map(|s| match &s.kind {
            SectionKind::CategorizedInlineList { categories } => {
                categories.iter().find(|c| c.id == cat_id).map(|c| c.enabled)
            }
            _ => None,
        });
        assert_eq!(cat_enabled, Some(false));
    }

    #[::core::prelude::v1::test]
    fn update_proposal_text_edits_bullet_summary_skills_and_clears_pii_warning() {
        let mut doc = crate::mocks::sample_root_document();
        let bullet_id = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::TimelineEntries { entries } => entries
                    .first()
                    .and_then(|e| e.bullets.first().map(|b| b.id)),
                _ => None,
            })
            .expect("bullet");
        let summary_id = if let Some(sec) = doc
            .body
            .sections
            .iter()
            .find(|s| matches!(s.kind, SectionKind::Summary { .. }))
        {
            sec.id
        } else {
            let id = Uuid::new_v4();
            doc.body.sections.insert(
                0,
                CvSection {
                    id,
                    origin_item_id: None,
                    title: "SUMMARY".to_string(),
                    enabled: true,
                    kind: SectionKind::Summary {
                        text: "Original summary".to_string(),
                    },
                },
            );
            id
        };
        let cat_id = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::CategorizedInlineList { categories } => {
                    categories.first().map(|c| c.id)
                }
                _ => None,
            })
            .expect("skill category");

        // 1. Bullet with a PII redaction placeholder gets unblocked once edited
        let mut bullet_prop = UiProposal::from_ai_proposal(
            &AiProposal::BulletRewrite {
                bullet_id,
                original: "Old bullet text".to_string(),
                proposed: "Contact [REDACTED_PII_EMAIL] for details".to_string(),
                rationale: "XYZ polish".to_string(),
            },
            &doc,
        );
        assert!(bullet_prop.pii_warning.is_some());
        assert!(bullet_prop.is_text_editable());
        assert!(update_proposal_text(&mut bullet_prop, "   ", Some(&doc)).is_err());

        update_proposal_text(
            &mut bullet_prop,
            "Identified 60+ critical vulnerabilities across HackerOne programs.",
            Some(&doc),
        )
        .expect("bullet edit");
        assert!(bullet_prop.is_edited);
        assert!(bullet_prop.pii_warning.is_none());
        assert_eq!(
            bullet_prop.proposed,
            "Identified 60+ critical vulnerabilities across HackerOne programs."
        );
        apply_ai_proposal(&mut doc, &bullet_prop.proposal);
        assert_eq!(
            doc.body.find_bullet(bullet_id).unwrap().text,
            "Identified 60+ critical vulnerabilities across HackerOne programs."
        );

        // 2. Summary rewrite edit
        let mut summary_prop = UiProposal::from_ai_proposal(
            &AiProposal::SummaryRewrite {
                section_id: summary_id,
                original: "Original summary".to_string(),
                proposed: "AI summary".to_string(),
            },
            &doc,
        );
        update_proposal_text(&mut summary_prop, "Custom edited summary.", Some(&doc))
            .expect("summary edit");
        apply_ai_proposal(&mut doc, &summary_prop.proposal);
        let updated_summary = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::Summary { text } if s.id == summary_id => Some(text.as_str()),
                _ => None,
            })
            .unwrap();
        assert_eq!(updated_summary, "Custom edited summary.");

        // 3. Skill reorder edit
        let mut skill_prop = UiProposal::from_ai_proposal(
            &AiProposal::SkillReorder {
                category_id: cat_id,
                items: vec!["Rust".to_string(), "Go".to_string()],
            },
            &doc,
        );
        update_proposal_text(&mut skill_prop, "Burp Suite, Rust, Python", Some(&doc))
            .expect("skill edit");
        apply_ai_proposal(&mut doc, &skill_prop.proposal);
        let updated_items = find_skill_category(&doc, cat_id).unwrap().items.clone();
        assert_eq!(updated_items, vec!["Burp Suite", "Rust", "Python"]);

        // 4. ToggleVisibility is not text-editable
        let mut vis_prop = UiProposal::from_ai_proposal(
            &AiProposal::ToggleVisibility {
                item_id: bullet_id,
                enabled: false,
                reason: "Off-topic".to_string(),
            },
            &doc,
        );
        assert!(!vis_prop.is_text_editable());
        assert!(update_proposal_text(&mut vis_prop, "anything", Some(&doc)).is_err());
    }
}