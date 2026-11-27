use super::*;
use crate::platform::{FileDialogFilter, SaveFileRequest};

impl WorkspaceView {
    // -----------------------------------------------------------------------
    // Official Sign in with ChatGPT Engine (Tailoring + Inline Bullet Rewrite)
    // -----------------------------------------------------------------------

    pub fn start_ai_tailoring_for_active(&mut self, cx: &mut Context<Self>) {
        self.launch_ai_tailoring_pass(None, "Before AI tailoring pass", cx);
    }

    /// One-click Page Compressor launched directly from the PDF page-overflow warning banner.
    pub fn run_page_compressor(&mut self, cx: &mut Context<Self>) {
        if self.ai_state.is_running {
            self.notify("An AI tailoring run is already in progress.");
            return;
        }
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a CV from the library first.");
            return;
        };
        if active.parent_id.is_none() {
            self.open_variation_dialog(Some(active.id), cx);
            self.variation_skills.insert("page_compressor".to_string());
            self.notify(
                "Master CVs are AI-protected. Create a variation with Page Compressor selected to fit 1 page.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            self.navigate_to(WorkspaceViewPage::Settings, cx);
            self.notify(
                "Connect ChatGPT or Google Gemini API in Settings to run Page Compressor.",
            );
            return;
        }
        if self.current_view != WorkspaceViewPage::Editor {
            self.split_preview_open = true;
            self.navigate_to(WorkspaceViewPage::Editor, cx);
        }
        self.launch_ai_tailoring_pass(
            Some(vec!["page_compressor".to_string()]),
            "Before Page Compressor AI pass",
            cx,
        );
    }

    fn launch_ai_tailoring_pass(
        &mut self,
        skill_ids_override: Option<Vec<String>>,
        snapshot_label: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a variation CV first.");
            return;
        };
        if active.parent_id.is_none() {
            self.notify_error(
                "AI tailoring is blocked on Master CVs. Branch a tailored variation first.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            let msg = match self.active_provider {
                AiProviderKind::ChatGpt => {
                    if self.chatgpt_auth.signed_in && !self.chatgpt_auth.plan_usage_enabled {
                        "ChatGPT plan usage is disabled. Open Settings to enable ChatGPT plan usage."
                    } else {
                        "Sign in with ChatGPT (or switch to Google Gemini API) in Settings to run AI tailoring."
                    }
                }
                AiProviderKind::GeminiApi => {
                    "Enter and save your Google Gemini API key (or switch to ChatGPT) in Settings to run AI tailoring."
                }
            };
            self.notify_error(msg);
            return;
        }
        if self.ai_state.is_running {
            self.notify("An AI tailoring run is already in progress.");
            return;
        }

        self.flush_pending_now();

        // 1. Pre-AI recovery snapshot
        if let Err(err) = snapshots::snapshot_before_ai(
            self.services.repository.as_ref(),
            active.id,
            snapshot_label,
        ) {
            tracing::warn!("Pre-AI snapshot failed: {err}");
        } else {
            self.refresh_snapshots();
        }

        // 2. Zero-trust PII scrubbing
        let payload = match self
            .services
            .privacy
            .scrub(&active.body, &active.pii_header)
        {
            Ok(p) => p,
            Err(err) => {
                self.notify_error(format!("PII scrubber failed: {err}"));
                return;
            }
        };

        let mut job_context = active.job_context.clone().unwrap_or_default();
        let skill_ids = if let Some(override_ids) = skill_ids_override {
            if job_context.job_description_raw.trim().is_empty() {
                job_context.job_description_raw =
                    "Compress verbose bullets and hide lowest-priority entries so the CV fits cleanly on 1 A4 page while preserving all metrics and factual accuracy.".to_string();
            }
            override_ids
        } else if job_context.applied_skill_ids.is_empty() {
            self.services
                .skills
                .list()
                .into_iter()
                .filter(|s| s.enabled)
                .map(|s| s.id)
                .collect()
        } else {
            job_context.applied_skill_ids.clone()
        };

        let request = VariationRequest {
            child_cv_id: active.id,
            job_context,
            skill_ids,
        };

        let doc_id = active.id;
        let redactions = payload.redaction_count;
        let active_model = self.active_model_id().to_string();
        self.ai_state = AiAgentState {
            is_running: true,
            target_doc_id: Some(doc_id),
            step: format!(
                "Scrubbed ({redactions} PII tokens redacted). Streaming proposals from {active_model}…"
            ),
            streamed_text: String::new(),
            error: None,
        };

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<AiEvent>();

        crate::runtime::handle().spawn(async move {
            let _ = ai.generate_variation(request, payload, tx).await;
        });

        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let is_terminal = matches!(
                    event,
                    AiEvent::RunCompleted { .. } | AiEvent::RunFailed { .. }
                );
                let _ = this.update(cx, |view, cx| {
                    view.handle_ai_event(doc_id, event, cx);
                    cx.notify();
                });
                if is_terminal {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn find_bullet_text(&self, section_id: ItemId, bullet_id: ItemId) -> Option<String> {
        let doc = self.active_document()?;
        let sec = doc.body.sections.iter().find(|s| s.id == section_id)?;
        match &sec.kind {
            SectionKind::TimelineEntries { entries } => entries.iter().find_map(|entry| {
                entry
                    .bullets
                    .iter()
                    .find(|b| b.id == bullet_id)
                    .map(|b| b.text.clone())
            }),
            SectionKind::SimpleBulletList { items } => items
                .iter()
                .find(|b| b.id == bullet_id)
                .map(|b| b.text.clone()),
            _ => None,
        }
    }

    pub fn open_rewrite_bullet_dialog(
        &mut self,
        section_id: ItemId,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a variation CV first.");
            return;
        };
        if active.parent_id.is_none() {
            self.notify_error(
                "AI rewrites are only allowed on tailored variations. Master CVs stay protected.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            let msg = match self.active_provider {
                AiProviderKind::ChatGpt => {
                    if self.chatgpt_auth.signed_in && !self.chatgpt_auth.plan_usage_enabled {
                        "Enable ChatGPT plan usage in Settings to rewrite achievements with AI."
                    } else {
                        "Sign in with ChatGPT (or switch to Google Gemini API) in Settings to rewrite achievements with AI."
                    }
                }
                AiProviderKind::GeminiApi => {
                    "Enter and save your Google Gemini API key in Settings to rewrite achievements with AI."
                }
            };
            self.notify_error(msg);
            return;
        }
        if self.ai_state.is_running {
            self.notify("Wait for the current AI task to finish.");
            return;
        }

        let enabled_skills: Vec<SkillManifest> = self
            .services
            .skills
            .list()
            .into_iter()
            .filter(|s| s.enabled)
            .collect();
        let enabled_ids: HashSet<String> = enabled_skills.iter().map(|s| s.id.clone()).collect();

        self.rewrite_selected_skills.clear();
        if let Some(jc) = active.job_context.as_ref() {
            for id in &jc.applied_skill_ids {
                if enabled_ids.contains(id) {
                    self.rewrite_selected_skills.insert(id.clone());
                }
            }
        }
        if self.rewrite_selected_skills.is_empty() {
            for s in &enabled_skills {
                if s.default_enabled {
                    self.rewrite_selected_skills.insert(s.id.clone());
                }
            }
        }
        if self.rewrite_selected_skills.is_empty() {
            for s in &enabled_skills {
                self.rewrite_selected_skills.insert(s.id.clone());
            }
        }

        self.rewrite_instruction_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        self.active_dialog = Some(Dialog::RewriteBullet {
            section_id,
            bullet_id,
        });
    }

    pub fn toggle_rewrite_skill(&mut self, skill_id: &str) {
        if self.rewrite_selected_skills.contains(skill_id) {
            self.rewrite_selected_skills.remove(skill_id);
        } else {
            self.rewrite_selected_skills.insert(skill_id.to_string());
        }
    }

    pub fn submit_bullet_rewrite_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::RewriteBullet {
            section_id,
            bullet_id,
        }) = self.active_dialog
        else {
            return;
        };
        let custom_instruction = self
            .rewrite_instruction_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let mut skill_ids: Vec<String> = self.rewrite_selected_skills.iter().cloned().collect();
        skill_ids.sort();
        self.active_dialog = None;
        self.request_bullet_rewrite_with_options(
            section_id,
            bullet_id,
            (!custom_instruction.is_empty()).then_some(custom_instruction),
            skill_ids,
            cx,
        );
    }

    pub fn request_bullet_rewrite_with_options(
        &mut self,
        _sec_id: ItemId,
        bullet_id: ItemId,
        custom_instruction: Option<String>,
        skill_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a variation CV first.");
            return;
        };
        if active.parent_id.is_none() {
            self.notify_error(
                "AI rewrites are only allowed on tailored variations. Master CVs stay protected.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            self.notify_error("Configure ChatGPT or Google Gemini API in Settings first.");
            return;
        }
        if self.ai_state.is_running {
            self.notify("Wait for the current AI task to finish.");
            return;
        }

        self.flush_pending_now();

        let payload = match self
            .services
            .privacy
            .scrub(&active.body, &active.pii_header)
        {
            Ok(p) => p,
            Err(err) => {
                self.notify_error(format!("PII scrubber failed: {err}"));
                return;
            }
        };

        let target_role = active
            .job_context
            .as_ref()
            .map(|jc| jc.target_role.clone())
            .unwrap_or_default();
        let instruction = custom_instruction.unwrap_or_else(|| {
            if target_role.is_empty() {
                "Rewrite this bullet for stronger impact using the Google XYZ formula while preserving all factual details and metrics."
                    .to_string()
            } else {
                format!(
                    "Rewrite this bullet for a {target_role} role using the Google XYZ formula. Preserve all facts and metrics."
                )
            }
        });

        let request = InlineRewriteRequest {
            cv_id: active.id,
            bullet_id,
            instruction,
            skill_ids,
        };

        let doc_id = active.id;
        self.ai_state = AiAgentState {
            is_running: true,
            target_doc_id: Some(doc_id),
            step: "Rewriting achievement via zero-trust AI…".to_string(),
            streamed_text: String::new(),
            error: None,
        };

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<AiEvent>();

        crate::runtime::handle().spawn(async move {
            let _ = ai.rewrite_bullet(request, payload, tx).await;
        });

        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let is_terminal = matches!(
                    event,
                    AiEvent::RunCompleted { .. } | AiEvent::RunFailed { .. }
                );
                let _ = this.update(cx, |view, cx| {
                    view.handle_ai_event(doc_id, event, cx);
                    cx.notify();
                });
                if is_terminal {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_ai_event(&mut self, doc_id: DocumentId, event: AiEvent, cx: &mut Context<Self>) {
        match event {
            AiEvent::RunStarted { .. } => {
                self.ai_state.is_running = true;
                self.ai_state.error = None;
            }
            AiEvent::Status { message } => {
                self.ai_state.step = message;
            }
            AiEvent::TextDelta { delta } => {
                self.ai_state.streamed_text.push_str(&delta);
            }
            AiEvent::ToolCall { name, .. } => {
                self.ai_state.step = format!("Evaluating tool: {name}…");
            }
            AiEvent::Proposal { proposal } => {
                if let Some(doc) = self.find_document(doc_id).cloned() {
                    let ui_prop = UiProposal::from_ai_proposal(&proposal, &doc);
                    let target_id = ui_prop.target_item_id;
                    let list = self.proposals.entry(doc_id).or_default();
                    if let Some(editing_id) = self.editing_proposal_id.as_deref()
                        && list.iter().any(|e| e.target_item_id == target_id && e.id == editing_id)
                    {
                        self.editing_proposal_id = None;
                    }
                    list.retain(|existing| existing.target_item_id != target_id);
                    list.push(ui_prop);
                    self.expand_containers_for_item(&doc, target_id);
                }
            }
            AiEvent::AtsScore { score } => {
                let numeric = score.score;
                if let Some(doc) = self.documents.iter_mut().find(|d| d.id == doc_id) {
                    self.ats_scores.insert(doc_id, score);
                    if let Some(ref mut jc) = doc.job_context {
                        jc.ats_match_score = Some(numeric);
                    }
                    let _ = self.services.repository.save(doc);
                }
            }
            AiEvent::RunCompleted { summary, .. } => {
                self.ai_state.is_running = false;
                if !summary.trim().is_empty() {
                    self.ai_summaries.insert(doc_id, summary.clone());
                }
                self.ai_state.step = summary;
                let count = self.proposals.get(&doc_id).map(|v| v.len()).unwrap_or(0);
                if count > 0 {
                    self.notify(format!(
                        "AI review ready: {count} {} awaiting your approval.",
                        if count == 1 { "proposal" } else { "proposals" }
                    ));
                } else {
                    self.notify("AI tailoring run completed.");
                }
            }
            AiEvent::RunFailed { message, .. } => {
                self.ai_state.is_running = false;
                self.ai_state.error = Some(message.clone());
                self.chatgpt_auth = self.services.ai.auth_status();
                if crate::ai::is_ai_limit_error(&message) {
                    let lower = message.to_ascii_lowercase();
                    if lower.starts_with("api") || lower.starts_with("usage limit") {
                        self.notify_error(message);
                    } else {
                        self.notify_error(format!("API limit reached: {message}"));
                    }
                } else {
                    self.notify_error(format!("AI error: {message}"));
                }
            }
        }
        let _ = cx;
    }

    pub fn resolve_proposal(&mut self, prop_id: &str, accept: bool, cx: &mut Context<Self>) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let prop = self
            .proposals
            .get(&active_id)
            .and_then(|list| list.iter().find(|p| p.id == prop_id).cloned());

        let Some(proposal) = prop else {
            return;
        };

        if accept {
            if let Some(warn) = &proposal.pii_warning {
                self.notify_error(warn.clone());
                return;
            }
            self.flush_pending_now();
            if let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) {
                let mut candidate = doc.clone();
                apply_ai_proposal(&mut candidate, &proposal.proposal);
                match self.services.repository.save_ai_batch(&candidate) {
                    Ok(()) => {
                        if let Ok(reloaded) = self.services.repository.load(active_id) {
                            *doc = reloaded;
                        } else {
                            *doc = candidate;
                        }
                        if let Some(list) = self.proposals.get_mut(&active_id) {
                            list.retain(|p| p.id != prop_id);
                        }
                        if self.editing_proposal_id.as_deref() == Some(prop_id) {
                            self.editing_proposal_id = None;
                        }
                        self.trigger_pdf_preview_render(cx);
                        self.notify("Change accepted and saved. Master CV is unchanged.");
                    }
                    Err(err) => {
                        self.notify_error(format!("Could not save AI change: {err}"));
                    }
                }
            }
        } else {
            if let Some(list) = self.proposals.get_mut(&active_id) {
                list.retain(|p| p.id != prop_id);
            }
            if self.editing_proposal_id.as_deref() == Some(prop_id) {
                self.editing_proposal_id = None;
            }
            self.notify("Proposal reverted.");
        }
    }

    pub fn begin_editing_proposal(
        &mut self,
        prop_id: &str,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let Some(prop) = self
            .proposals
            .get(&active_id)
            .and_then(|list| list.iter().find(|p| p.id == prop_id))
            .cloned()
        else {
            return;
        };
        if !prop.is_text_editable() {
            return;
        }
        self.editing_proposal_id = Some(prop.id.clone());
        let initial_text = prop.proposed.clone();
        self.proposal_edit_input.update(cx, |input, cx| {
            input.set_value(initial_text);
            cx.notify();
        });
        if let Some(window) = window {
            let focus_handle = self.proposal_edit_input.read(cx).focus_handle(cx);
            window.focus(&focus_handle);
        }
    }

    pub fn cancel_editing_proposal(&mut self) {
        self.editing_proposal_id = None;
    }

    pub fn save_edited_proposal(
        &mut self,
        prop_id: &str,
        accept_immediately: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let edited_text = self.proposal_edit_input.read(cx).text().to_string();
        let doc_snapshot = self.find_document(active_id).cloned();

        let update_res = self
            .proposals
            .get_mut(&active_id)
            .and_then(|list| list.iter_mut().find(|p| p.id == prop_id))
            .map(|prop| update_proposal_text(prop, &edited_text, doc_snapshot.as_ref()));

        match update_res {
            Some(Ok(())) => {
                self.editing_proposal_id = None;
                if accept_immediately {
                    self.resolve_proposal(prop_id, true, cx);
                } else {
                    self.notify("Proposal draft updated. Review the diff or click Accept change.");
                }
            }
            Some(Err(msg)) => {
                self.notify_error(msg);
            }
            None => {
                self.editing_proposal_id = None;
            }
        }
    }

    pub fn accept_all_proposals(&mut self, cx: &mut Context<Self>) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let list = self.proposals.get(&active_id).cloned().unwrap_or_default();
        if list.is_empty() {
            return;
        }

        let mut safe = Vec::new();
        let mut blocked = Vec::new();
        for item in list {
            if item.pii_warning.is_some() {
                blocked.push(item);
            } else {
                safe.push(item);
            }
        }

        if safe.is_empty() {
            self.notify_error(
                "All pending proposals were blocked because they contain redaction placeholders.",
            );
            return;
        }

        self.flush_pending_now();

        if let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) {
            let mut candidate = doc.clone();
            for item in &safe {
                apply_ai_proposal(&mut candidate, &item.proposal);
            }
            match self.services.repository.save_ai_batch(&candidate) {
                Ok(()) => {
                    if let Ok(reloaded) = self.services.repository.load(active_id) {
                        *doc = reloaded;
                    } else {
                        *doc = candidate;
                    }
                    self.editing_proposal_id = None;
                    if blocked.is_empty() {
                        self.proposals.remove(&active_id);
                        self.notify(format!(
                            "Accepted {} {}. Master CV is unchanged.",
                            safe.len(),
                            if safe.len() == 1 { "change" } else { "changes" }
                        ));
                    } else {
                        self.proposals.insert(active_id, blocked.clone());
                        self.notify(format!(
                            "Accepted {} changes; {} skipped due to PII placeholder safety guard.",
                            safe.len(),
                            blocked.len()
                        ));
                    }
                    self.trigger_pdf_preview_render(cx);
                }
                Err(err) => {
                    self.notify_error(format!("Could not apply AI batch: {err}"));
                }
            }
        }
    }

    pub fn dismiss_all_proposals(&mut self) {
        if let Some(active_id) = self.active_doc_id {
            self.proposals.remove(&active_id);
            self.editing_proposal_id = None;
            self.notify("Dismissed pending AI proposals.");
        }
    }

    // -----------------------------------------------------------------------
    // Compare with Master & Promote to Master (Real Backend)
    // -----------------------------------------------------------------------

    pub fn open_compare_dialog(&mut self) {
        self.flush_pending_now();
        self.active_dialog = Some(Dialog::Compare);
    }

    pub fn calculate_lineage_changes(&self) -> Vec<ItemDiff> {
        let Some(doc) = self.active_document() else {
            return Vec::new();
        };
        let Some(parent) = doc.parent_id.and_then(|pid| self.find_document(pid)) else {
            return Vec::new();
        };
        lineage_diff::compute_diff(doc, parent)
            .map(|diff| diff.item_diffs)
            .unwrap_or_default()
    }

    pub fn open_promote_dialog(
        &mut self,
        sec_id: ItemId,
        item_id: ItemId,
        entry_id: Option<ItemId>,
    ) {
        self.promote_target = Some((sec_id, item_id, entry_id));
        self.active_dialog = Some(Dialog::PromoteItem);
    }

    pub fn describe_promote_target(&self) -> Option<String> {
        let (_, item_id, _) = self.promote_target?;
        Some(describe_item_in_doc(self.active_document()?, item_id))
    }

    fn expand_containers_for_item(&mut self, doc: &CvDocument, item_id: ItemId) {
        for sec in &doc.body.sections {
            if sec.id == item_id {
                self.expanded_sections.insert(sec.id, true);
                return;
            }
            match &sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    for entry in entries {
                        if entry.id == item_id || entry.bullets.iter().any(|b| b.id == item_id) {
                            self.expanded_sections.insert(sec.id, true);
                            self.expanded_entries.insert(entry.id, true);
                            return;
                        }
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if items.iter().any(|b| b.id == item_id) {
                        self.expanded_sections.insert(sec.id, true);
                        return;
                    }
                }
                SectionKind::CategorizedInlineList { categories } => {
                    if categories.iter().any(|c| c.id == item_id) {
                        self.expanded_sections.insert(sec.id, true);
                        return;
                    }
                }
                SectionKind::Summary { .. } => {}
            }
        }
    }

    pub(super) fn prune_orphaned_proposals_for_active(&mut self) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let valid_ids: HashSet<ItemId> = {
            let Some(doc) = self.active_document() else {
                return;
            };
            let mut ids = HashSet::new();
            for sec in &doc.body.sections {
                ids.insert(sec.id);
                match &sec.kind {
                    SectionKind::TimelineEntries { entries } => {
                        for e in entries {
                            ids.insert(e.id);
                            for b in &e.bullets {
                                ids.insert(b.id);
                            }
                        }
                    }
                    SectionKind::SimpleBulletList { items } => {
                        for b in items {
                            ids.insert(b.id);
                        }
                    }
                    SectionKind::CategorizedInlineList { categories } => {
                        for cat in categories {
                            ids.insert(cat.id);
                        }
                    }
                    SectionKind::Summary { .. } => {}
                }
            }
            ids
        };
        if let Some(list) = self.proposals.get_mut(&active_id) {
            list.retain(|p| valid_ids.contains(&p.target_item_id));
            if list.is_empty() {
                self.proposals.remove(&active_id);
            }
        }
    }

    pub fn execute_promote(&mut self, cx: &mut Context<Self>) {
        let Some((_, item_id, _)) = self.promote_target.take() else {
            self.active_dialog = None;
            return;
        };

        let Some(child_id) = self.active_doc_id else {
            self.active_dialog = None;
            return;
        };
        self.flush_pending_now();

        match self.services.repository.promote_item(child_id, item_id) {
            Ok(outcome) => {
                // Reload both parent & child so origin_item_id links stay in sync
                if let Ok(updated_parent) =
                    self.services.repository.load(outcome.parent_document_id)
                    && let Some(slot) = self
                        .documents
                        .iter_mut()
                        .find(|d| d.id == outcome.parent_document_id)
                {
                    *slot = updated_parent;
                }
                if let Ok(updated_child) = self.services.repository.load(child_id)
                    && let Some(slot) = self.documents.iter_mut().find(|d| d.id == child_id)
                {
                    *slot = updated_child;
                }
                self.active_dialog = None;
                self.trigger_pdf_preview_render(cx);
                self.notify("Item promoted into your Master CV.");
            }
            Err(err) => {
                self.active_dialog = None;
                self.notify_error(format!("Promotion failed: {err}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Payload Preview & Non-Blocking PDF Export
    // -----------------------------------------------------------------------

    pub fn open_payload_dialog(&mut self) {
        if self.active_document().is_none() {
            self.notify("Select a CV from the library to inspect its scrubbed AI payload.");
            return;
        }
        self.active_dialog = Some(Dialog::PayloadPreview);
    }

    pub fn build_payload_preview(&self) -> (String, usize) {
        let Some(doc) = self.active_document() else {
            return ("{}".to_string(), 0);
        };
        match self.services.privacy.scrub(&doc.body, &doc.pii_header) {
            Ok(payload) => {
                let pretty = serde_json::from_str::<serde_json::Value>(&payload.json)
                    .and_then(|v| serde_json::to_string_pretty(&v))
                    .unwrap_or(payload.json);
                (pretty, payload.redaction_count)
            }
            Err(err) => (format!("{{\"error\": \"{err}\"}}"), 0),
        }
    }

    pub fn open_export_dialog(&mut self) {
        if self.active_document().is_none() {
            self.notify("Select a CV from the library before exporting to PDF.");
            return;
        }
        self.active_dialog = Some(Dialog::ExportPdf);
    }

    pub(super) fn initial_save_dir() -> PathBuf {
        let default_dir = exporter::default_export_dir();
        if default_dir.is_dir() {
            return default_dir;
        }
        if let Some(docs_dir) = dirs::document_dir()
            && docs_dir.is_dir()
        {
            return docs_dir;
        }
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    pub fn execute_export_pdf(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_exporting_pdf {
            return;
        }
        self.flush_pending_now();
        let Some(doc) = self.active_document().cloned() else {
            self.active_dialog = None;
            self.notify("Select a CV from the library before exporting to PDF.");
            return;
        };
        let default_name = exporter::export_filename(&doc);
        let initial_dir = Self::initial_save_dir();
        let pdf_engine = self.services.pdf.clone();

        self.active_dialog = None;
        self.is_exporting_pdf = true;
        self.notify("Choose where to save your PDF…");

        let path_rx = Self::prompt_save_single_file(
            SaveFileRequest {
                title: "Export CV as PDF".to_string(),
                initial_dir: Some(initial_dir),
                default_name: Some(default_name),
                default_extension: Some("pdf".to_string()),
                filters: vec![FileDialogFilter {
                    label: "PDF Documents (*.pdf)",
                    extensions: &["pdf"],
                }],
            },
            cx,
        );

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => {
                    let _ = this.update(cx, |view, cx| {
                        view.is_exporting_pdf = false;
                        view.clear_notice();
                        cx.notify();
                    });
                    return;
                }
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.is_exporting_pdf = false;
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("pdf");
            }

            let write_result = cx
                .background_executor()
                .spawn(async move {
                    let bytes = pdf_engine.compile_to_pdf(&doc.pii_header, &doc.body)?;
                    std::fs::write(&path, bytes)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.is_exporting_pdf = false;
                match write_result {
                    Ok(path) => {
                        view.notify(format!("PDF exported to {}", path.display()));
                    }
                    Err(err) => {
                        view.notify_error(format!("PDF export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

}