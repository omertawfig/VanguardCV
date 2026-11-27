use super::*;
use crate::platform::{FileDialogFilter, OpenFileRequest};

impl WorkspaceView {
    // -----------------------------------------------------------------------
    // Master CV Importer (.docx & .pdf) with Interactive Verification
    // -----------------------------------------------------------------------

    pub fn open_import_cv_picker(&mut self, cx: &mut Context<Self>) {
        if self.import_loading {
            return;
        }
        let path_rx = Self::prompt_open_single_file(
            OpenFileRequest {
                title: "Import Master CV (.docx or .pdf)".to_string(),
                initial_dir: Some(Self::initial_save_dir()),
                filters: vec![
                    FileDialogFilter {
                        label: "CV Documents (*.docx, *.pdf)",
                        extensions: &["docx", "pdf"],
                    },
                    FileDialogFilter {
                        label: "Word Documents (*.docx)",
                        extensions: &["docx"],
                    },
                    FileDialogFilter {
                        label: "PDF Documents (*.pdf)",
                        extensions: &["pdf"],
                    },
                ],
            },
            cx,
        );

        cx.spawn(async move |this, cx| {
            let path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open file picker: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };

            let _ = this.update(cx, |view, cx| {
                view.import_loading = true;
                view.notify("Parsing CV document for verification…");
                cx.notify();
            });

            let parsed = cx
                .background_executor()
                .spawn(async move { ImportDraft::from_file(&path) })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.import_loading = false;
                match parsed {
                    Ok(draft) => {
                        let suggested_title = draft.suggested_title.clone();
                        view.import_title_input.update(cx, |input, cx| {
                            input.set_value(suggested_title);
                            cx.notify();
                        });
                        view.import_lines_scroll.set_offset(point(px(0.0), px(0.0)));
                        view.import_preview_scroll
                            .set_offset(point(px(0.0), px(0.0)));
                        view.import_draft = Some(draft);
                        view.active_dialog = Some(Dialog::ImportCv);
                        view.clear_notice();
                    }
                    Err(err) => {
                        view.notify_error(format!("Could not import CV: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_import_line_role(&mut self, index: usize, role: ImportLineRole) {
        if let Some(draft) = self.import_draft.as_mut() {
            draft.set_line_role(index, role);
        }
    }

    pub fn set_import_section_kind(&mut self, index: usize, kind: ImportSectionKind) {
        if let Some(draft) = self.import_draft.as_mut() {
            draft.set_section_kind(index, kind);
        }
    }

    pub fn merge_import_line_up(&mut self, index: usize) {
        if let Some(draft) = self.import_draft.as_mut() {
            draft.merge_line_up(index);
        }
    }

    pub fn skip_import_header_lines(&mut self) {
        if let Some(draft) = self.import_draft.as_mut() {
            draft.skip_all_above_first_section();
        }
    }

    pub fn reset_import_auto_detect(&mut self) {
        if let Some(draft) = self.import_draft.as_mut() {
            draft.reset_to_auto();
        }
    }

    pub fn scroll_import_lines_by(&mut self, dy: Pixels, cx: &mut Context<Self>) {
        let cur = self.import_lines_scroll.offset();
        let max_y = self.import_lines_scroll.max_offset().height;
        if max_y <= px(0.5) {
            return;
        }
        let next_y = (cur.y + dy).clamp(-max_y, px(0.0));
        if (next_y - cur.y).abs() > px(0.1) {
            self.import_lines_scroll.set_offset(point(cur.x, next_y));
            cx.notify();
        }
    }

    pub fn scroll_import_preview_by(&mut self, dy: Pixels, cx: &mut Context<Self>) {
        let cur = self.import_preview_scroll.offset();
        let max_y = self.import_preview_scroll.max_offset().height;
        if max_y <= px(0.5) {
            return;
        }
        let next_y = (cur.y + dy).clamp(-max_y, px(0.0));
        if (next_y - cur.y).abs() > px(0.1) {
            self.import_preview_scroll.set_offset(point(cur.x, next_y));
            cx.notify();
        }
    }

    pub fn confirm_import_master_cv(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.import_draft.as_ref() else {
            return;
        };
        let stats = draft.stats();
        if stats.sections == 0 {
            self.notify_error("Mark at least one line as a Section before importing.");
            return;
        }

        let title = self.import_title_input.read(cx).text().trim().to_string();
        let built = draft.build_master_document(title);
        let final_title = built.title.clone();

        // Personal info header is ALWAYS blank (`PiiContactHeader::default()`);
        // users enter contact details manually in Personal info.
        match self
            .services
            .repository
            .create_root(&built.title, built.pii_header, built.body)
        {
            Ok(doc) => {
                let doc_id = doc.id;
                self.documents.push(doc);
                self.active_doc_id = Some(doc_id);
                self.current_view = WorkspaceViewPage::Editor;
                self.active_dialog = None;
                self.import_draft = None;
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);
                self.notify(format!(
                    "Imported \"{final_title}\" as a Master CV. Enter your personal details in Personal info."
                ));
            }
            Err(err) => {
                self.notify_error(format!("Failed to save imported Master CV: {err}"));
            }
        }
    }

    pub fn open_variation_dialog(&mut self, root_id: Option<DocumentId>, cx: &mut Context<Self>) {
        let Some(default_root) = root_id.or_else(|| {
            self.active_document()
                .map(|active| active.parent_id.unwrap_or(active.id))
                .or_else(|| {
                    self.documents
                        .iter()
                        .find(|d| d.parent_id.is_none())
                        .map(|d| d.id)
                })
        }) else {
            self.notify("Create a master CV first before creating a variation.");
            return;
        };

        self.variation_company_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.variation_role_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.variation_jd_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        self.variation_skills.clear();
        for skill in self.services.skills.list() {
            if skill.enabled && skill.default_enabled {
                self.variation_skills.insert(skill.id);
            }
        }
        // If no skill had default_enabled, pre-select enabled skills
        if self.variation_skills.is_empty() {
            for skill in self.services.skills.list() {
                if skill.enabled {
                    self.variation_skills.insert(skill.id);
                }
            }
        }

        if self.available_models.is_empty() && !self.models_loading && self.is_ai_configured() {
            self.fetch_available_models(false, cx);
        }

        self.active_dialog = Some(Dialog::NewVariation {
            root_id: default_root,
        });
    }

    pub fn select_variation_root(&mut self, root_id: DocumentId) {
        if let Some(Dialog::NewVariation {
            root_id: ref mut current,
        }) = self.active_dialog
        {
            *current = root_id;
        }
    }

    pub fn create_variation_from_dialog(&mut self, cx: &mut Context<Self>) {
        let root_id = match self.active_dialog {
            Some(Dialog::NewVariation { root_id }) => root_id,
            _ => return,
        };

        self.flush_pending_now();

        let company = self
            .variation_company_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let role = self.variation_role_input.read(cx).text().trim().to_string();
        let jd = self.variation_jd_input.read(cx).text().trim().to_string();

        let title = if !company.is_empty() && !role.is_empty() {
            format!("{company} — {role}")
        } else if !company.is_empty() {
            format!("{company} — Variation")
        } else if !role.is_empty() {
            role.clone()
        } else {
            "Tailored Variation".to_string()
        };

        let applied_skill_ids: Vec<String> = self.variation_skills.iter().cloned().collect();
        let job_ctx = JobContext {
            company_name: company.clone(),
            target_role: role.clone(),
            job_description_raw: jd.clone(),
            application_url: None,
            applied_skill_ids: applied_skill_ids.clone(),
            ats_match_score: None,
        };

        match self
            .services
            .repository
            .branch_sub_variation(root_id, &title, job_ctx)
        {
            Ok(child) => {
                let child_id = child.id;
                self.documents.push(child);
                self.active_doc_id = Some(child_id);
                self.current_view = WorkspaceViewPage::Editor;
                self.active_dialog = None;
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);

                if self.services.ai.is_configured() && (!jd.is_empty() || !role.is_empty()) {
                    self.start_ai_tailoring_for_active(cx);
                } else if !self.services.ai.is_configured() {
                    self.notify(format!(
                        "Branched \"{title}\". Configure ChatGPT or Google Gemini in Settings to run automated tailoring."
                    ));
                } else {
                    self.notify(format!("Branched \"{title}\" from Master CV."));
                }
            }
            Err(err) => {
                self.notify_error(format!("Failed to branch variation: {err}"));
            }
        }
    }

    pub fn open_delete_cv_dialog(&mut self, doc_id: DocumentId) {
        self.active_dialog = Some(Dialog::ConfirmDeleteCv { doc_id });
    }

    pub fn delete_document(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        let target_title = self
            .find_document(id)
            .map(|d| d.title.clone())
            .unwrap_or_else(|| "CV".to_string());
        let child_ids: Vec<DocumentId> = self
            .documents
            .iter()
            .filter(|d| d.parent_id == Some(id))
            .map(|d| d.id)
            .collect();

        self.flush_pending_now();

        // Repository guards against deleting a master with active children,
        // so delete linked variations first in deterministic order.
        for child_id in &child_ids {
            if let Err(err) = self.services.repository.delete(*child_id) {
                self.active_dialog = None;
                self.reload_documents_from_db(cx);
                self.notify_error(format!("Could not delete linked variation: {err}"));
                return;
            }
        }

        match self.services.repository.delete(id) {
            Ok(()) => {
                let mut removed_ids = child_ids.clone();
                removed_ids.push(id);
                self.documents
                    .retain(|d| !removed_ids.contains(&d.id));
                for rid in &removed_ids {
                    self.proposals.remove(rid);
                    self.ats_scores.remove(rid);
                    self.ai_summaries.remove(rid);
                }
                if self
                    .ai_state
                    .target_doc_id
                    .is_some_and(|tid| removed_ids.contains(&tid))
                {
                    self.ai_state = AiAgentState::default();
                }
                if self
                    .default_cv_id
                    .is_some_and(|did| removed_ids.contains(&did))
                {
                    self.default_cv_id = None;
                    let _ = self
                        .services
                        .repository
                        .save_setting(settings_keys::DEFAULT_CV_ID, "");
                }
                for rid in &removed_ids {
                    self.job_cache.remove_cv_profiles(*rid);
                }
                self.persist_job_profiles_cache();
                if let Some(CvSearchSource::Single(sid)) = self.job_source
                    && removed_ids.contains(&sid)
                {
                    self.job_source = None;
                    self.job_profiles.clear();
                    self.job_profiles_from_cache = false;
                    self.job_profiles_used_fallback = false;
                    self.job_profiles_stale = false;
                }
                self.active_dialog = None;
                self.editing_proposal_id = None;
                if self
                    .active_doc_id
                    .is_some_and(|active_id| removed_ids.contains(&active_id))
                {
                    self.active_doc_id = None;
                    self.rendered_images.clear();
                    self.preview_error = None;
                    self.snapshots.clear();
                    if matches!(
                        self.current_view,
                        WorkspaceViewPage::Editor | WorkspaceViewPage::Preview
                    ) {
                        self.current_view = WorkspaceViewPage::Library;
                    }
                } else {
                    self.refresh_snapshots();
                }

                if child_ids.is_empty() {
                    self.notify(format!("Deleted \"{target_title}\"."));
                } else {
                    self.notify(format!(
                        "Deleted \"{target_title}\" and {} linked {}.",
                        child_ids.len(),
                        if child_ids.len() == 1 {
                            "variation"
                        } else {
                            "variations"
                        }
                    ));
                }
            }
            Err(err) => {
                self.active_dialog = None;
                self.reload_documents_from_db(cx);
                self.notify_error(format!("Delete failed: {err}"));
            }
        }
    }

}