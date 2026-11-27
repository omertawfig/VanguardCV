use super::*;

impl WorkspaceView {
    // -----------------------------------------------------------------------
    // Persistence & Live Preview Helpers
    // -----------------------------------------------------------------------

    /// Marks the active document dirty, schedules a debounced SQLite flush, and
    /// schedules a debounced Typst PDF preview refresh.
    pub fn persist_active_doc(&mut self, cx: &mut Context<Self>) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        if let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) {
            doc.version += 1;
            doc.updated_at = Utc::now();
            self.debounced_saver.schedule(doc.clone());
        }

        self.save_generation = self.save_generation.wrapping_add(1);
        let generation = self.save_generation;
        let saver = self.debounced_saver.clone();

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;

            let still_latest = this
                .read_with(cx, |view, _| view.save_generation == generation)
                .unwrap_or(false);
            if !still_latest {
                return;
            }

            let flush_result = cx
                .background_executor()
                .spawn(async move { saver.flush() })
                .await;

            let _ = this.update(cx, |view, cx| {
                if let Err(err) = flush_result {
                    view.notify_error(format!("Failed to save changes: {err}"));
                }
                view.trigger_pdf_preview_render(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Synchronously flushes any pending debounced edits to SQLite right now.
    pub fn flush_pending_now(&mut self) {
        if let Err(err) = self.debounced_saver.flush() {
            tracing::error!("Failed to flush pending edits to SQLite: {err}");
            self.notify_error(format!("Could not save pending changes: {err}"));
        }
    }

    /// Reloads all documents from SQLite, preserving `active_doc_id` if still present.
    pub fn reload_documents_from_db(&mut self, cx: &mut Context<Self>) {
        self.documents = load_all_documents(&self.services);
        if let Some(active_id) = self.active_doc_id
            && !self.documents.iter().any(|d| d.id == active_id)
        {
            self.active_doc_id = None;
            self.rendered_images.clear();
            self.preview_error = None;
        }
        self.refresh_snapshots();
        if self.active_doc_id.is_some() {
            self.trigger_pdf_preview_render(cx);
        }
    }

    pub fn refresh_snapshots(&mut self) {
        let Some(active_id) = self.active_doc_id else {
            self.snapshots.clear();
            return;
        };
        let mut list = self
            .services
            .repository
            .list_snapshots(active_id)
            .unwrap_or_default();
        list.reverse(); // newest first for UI display
        self.snapshots = list;
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    pub fn active_document(&self) -> Option<&CvDocument> {
        let active_id = self.active_doc_id?;
        self.documents.iter().find(|d| d.id == active_id)
    }

    pub fn find_document(&self, id: DocumentId) -> Option<&CvDocument> {
        self.documents.iter().find(|d| d.id == id)
    }

    pub fn active_proposals(&self) -> Vec<UiProposal> {
        self.active_doc_id
            .and_then(|id| self.proposals.get(&id))
            .cloned()
            .unwrap_or_default()
    }

    pub fn active_ats_score(&self) -> Option<&AtsScore> {
        self.active_doc_id.and_then(|id| self.ats_scores.get(&id))
    }

    pub fn active_ai_summary(&self) -> Option<&str> {
        self.active_doc_id
            .and_then(|id| self.ai_summaries.get(&id))
            .map(String::as_str)
    }

    pub fn dismiss_active_ai_summary(&mut self) {
        if let Some(id) = self.active_doc_id {
            self.ai_summaries.remove(&id);
        }
    }

    /// Scrolls the Settings available-models list vertically, returning `true`
    /// when the scroll offset actually moved (so nested scroll handlers know
    /// whether to stop propagation to the outer Settings page).
    pub fn scroll_models_list_by(&mut self, dy: Pixels, cx: &mut Context<Self>) -> bool {
        let cur = self.models_list_scroll.offset();
        let max_y = self.models_list_scroll.max_offset().height;
        if max_y <= px(0.5) {
            return false;
        }
        let next_y = (cur.y + dy).clamp(-max_y, px(0.0));
        if (next_y - cur.y).abs() > px(0.1) {
            self.models_list_scroll.set_offset(point(cur.x, next_y));
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn reset_models_list_scroll(&self) {
        self.models_list_scroll.set_offset(point(px(0.0), px(0.0)));
    }

    pub fn find_proposal_for_item(&self, item_id: ItemId, _text: &str) -> Option<UiProposal> {
        let active_id = self.active_doc_id?;
        self.proposals
            .get(&active_id)?
            .iter()
            .find(|p| p.target_item_id == item_id)
            .cloned()
    }

    pub fn active_skills(&self) -> Vec<SkillManifest> {
        self.services.skills.list()
    }

    pub fn is_user_owned_skill(&self, skill: &SkillManifest) -> bool {
        let Some(src) = skill.source_path.as_ref() else {
            return false;
        };
        let user_dir = self.services.skills.skills_dir();
        let canon_user = user_dir.canonicalize().unwrap_or(user_dir);
        let canon_src = src.canonicalize().unwrap_or_else(|_| src.clone());
        canon_src.starts_with(&canon_user)
    }

    pub fn is_ai_configured(&self) -> bool {
        self.services.ai.is_configured()
    }

    pub fn active_model_id(&self) -> &str {
        match self.active_provider {
            AiProviderKind::ChatGpt => self.selected_model.as_str(),
            AiProviderKind::GeminiApi => self.selected_gemini_model.as_str(),
        }
    }

    pub fn active_provider_name(&self) -> &'static str {
        self.active_provider.display_name()
    }

    pub fn is_sidebar_visible(&self) -> bool {
        self.sidebar_user_toggle
            .unwrap_or(self.viewport_width >= 860.0)
    }

    pub fn toggle_sidebar(&mut self) {
        let currently_visible = self.is_sidebar_visible();
        self.sidebar_user_toggle = Some(!currently_visible);
    }

    pub fn is_compact(&self) -> bool {
        self.viewport_width < 920.0
    }

    pub fn is_narrow(&self) -> bool {
        self.viewport_width < 700.0
    }

    pub fn content_width(&self) -> f32 {
        let sidebar_w = if self.is_sidebar_visible() {
            if self.viewport_width < 1020.0 {
                204.0
            } else {
                228.0
            }
        } else {
            0.0
        };
        (self.viewport_width - sidebar_w).max(320.0)
    }

    pub fn page_padding_x(&self) -> Pixels {
        if self.viewport_width < 680.0 {
            px(14.0)
        } else if self.viewport_width < 960.0 {
            px(24.0)
        } else {
            px(40.0)
        }
    }

    pub fn can_show_split_preview(&self) -> bool {
        self.content_width() >= 900.0
    }

    pub fn is_split_preview_active(&self) -> bool {
        self.current_view == WorkspaceViewPage::Editor
            && self.active_document().is_some()
            && self.split_preview_open
            && self.can_show_split_preview()
    }

    pub fn split_preview_pane_width(&self) -> f32 {
        (self.content_width() * 0.42).clamp(360.0, 520.0)
    }

    pub fn editor_pane_width(&self) -> f32 {
        if self.is_split_preview_active() {
            (self.content_width() - self.split_preview_pane_width()).max(360.0)
        } else {
            self.content_width()
        }
    }

    pub fn editor_padding_x(&self) -> Pixels {
        if self.is_split_preview_active() {
            let w = self.editor_pane_width();
            if w < 560.0 {
                px(16.0)
            } else if w < 740.0 {
                px(22.0)
            } else {
                px(32.0)
            }
        } else {
            self.page_padding_x()
        }
    }

    pub fn toggle_split_preview(&mut self, cx: &mut Context<Self>) {
        if self.can_show_split_preview() {
            self.split_preview_open = !self.split_preview_open;
            if self.split_preview_open {
                self.flush_pending_now();
                self.trigger_pdf_preview_render(cx);
            }
        } else {
            self.navigate_to(WorkspaceViewPage::Preview, cx);
        }
    }

    pub fn preview_scroll_handle(&self, is_split: bool) -> &ScrollHandle {
        if is_split {
            &self.split_preview_scroll
        } else {
            &self.full_preview_scroll
        }
    }

    pub fn begin_preview_pan(&mut self, is_split: bool, mouse_pos: Point<Pixels>) {
        let start_offset = self.preview_scroll_handle(is_split).offset();
        self.preview_pan_state = Some(PreviewPanState {
            is_split,
            start_mouse: mouse_pos,
            start_offset,
        });
    }

    pub fn update_preview_pan(&mut self, mouse_pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(pan) = self.preview_pan_state else {
            return;
        };
        let handle = self.preview_scroll_handle(pan.is_split);
        let max = handle.max_offset();
        let dx = mouse_pos.x - pan.start_mouse.x;
        let dy = mouse_pos.y - pan.start_mouse.y;
        let next_x = (pan.start_offset.x + dx).clamp(-max.width, px(0.0));
        let next_y = (pan.start_offset.y + dy).clamp(-max.height, px(0.0));
        handle.set_offset(point(next_x, next_y));
        cx.notify();
    }

    pub fn end_preview_pan(&mut self) {
        self.preview_pan_state = None;
    }

    pub fn scroll_preview_by(
        &mut self,
        is_split: bool,
        dx: Pixels,
        dy: Pixels,
        cx: &mut Context<Self>,
    ) {
        let handle = self.preview_scroll_handle(is_split);
        let cur = handle.offset();
        let max = handle.max_offset();
        let next_x = (cur.x + dx).clamp(-max.width, px(0.0));
        let next_y = (cur.y + dy).clamp(-max.height, px(0.0));
        if next_x != cur.x || next_y != cur.y {
            handle.set_offset(point(next_x, next_y));
            cx.notify();
        }
    }

    pub fn clamp_preview_scroll(&self, is_split: bool) {
        let handle = self.preview_scroll_handle(is_split);
        let cur = handle.offset();
        let max = handle.max_offset();
        let clamped_x = cur.x.clamp(-max.width, px(0.0));
        let clamped_y = cur.y.clamp(-max.height, px(0.0));
        handle.set_offset(point(clamped_x, clamped_y));
    }

    // -----------------------------------------------------------------------
    // Notifications & Navigation
    // -----------------------------------------------------------------------

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.notice = Some(msg.into());
        self.notice_is_error = false;
        self.notice_generation = self.notice_generation.wrapping_add(1);
    }

    pub fn notify_error(&mut self, msg: impl Into<String>) {
        self.notice = Some(msg.into());
        self.notice_is_error = true;
        self.notice_generation = self.notice_generation.wrapping_add(1);
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
        self.notice_is_error = false;
        self.notice_generation = self.notice_generation.wrapping_add(1);
    }

    pub fn navigate_to(&mut self, page: WorkspaceViewPage, cx: &mut Context<Self>) {
        if page == WorkspaceViewPage::Preview {
            self.flush_pending_now();
            self.trigger_pdf_preview_render(cx);
        } else if page == WorkspaceViewPage::Editor
            && self.split_preview_open
            && self.rendered_images.is_empty()
        {
            self.trigger_pdf_preview_render(cx);
        } else if page == WorkspaceViewPage::Settings {
            self.refresh_snapshots();
            if self.available_models.is_empty() && !self.models_loading && self.is_ai_configured() {
                self.fetch_available_models(false, cx);
            }
        } else if page == WorkspaceViewPage::Jobs {
            self.sync_job_source_and_cached_profiles(cx);
        }
        self.job_country_dropdown_open = false;
        self.current_view = page;
    }

    pub fn open_document(
        &mut self,
        id: DocumentId,
        page: WorkspaceViewPage,
        cx: &mut Context<Self>,
    ) {
        self.flush_pending_now();
        self.active_doc_id = Some(id);
        self.current_view = page;
        self.refresh_snapshots();
        self.trigger_pdf_preview_render(cx);
    }

    pub fn select_document(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        self.flush_pending_now();
        self.active_doc_id = Some(id);
        self.editing_proposal_id = None;
        self.refresh_snapshots();
        self.trigger_pdf_preview_render(cx);
    }

    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme = match mode {
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::Light => Theme::light(),
        };
        theme::set_active_theme_mode(mode);
        let value = serde_json::json!({
            "theme": match mode {
                ThemeMode::Dark => "dark",
                ThemeMode::Light => "light",
            }
        });
        let _ = self
            .services
            .repository
            .save_setting(settings_keys::UI_PREFERENCES, &value.to_string());
    }

    pub fn toggle_theme(&mut self) {
        let next = match self.theme.mode {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        };
        self.set_theme_mode(next);
    }

    pub fn close_dialog(&mut self) {
        if matches!(self.active_dialog, Some(Dialog::ImportCv)) {
            self.import_draft = None;
        }
        self.active_dialog = None;
    }

    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.library_query.clear();
        self.search_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
    }

    // -----------------------------------------------------------------------
    // Library & Master / Variation Creation
    // -----------------------------------------------------------------------

    pub fn open_new_master_dialog(&mut self, cx: &mut Context<Self>) {
        self.master_title_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::NewMaster);
    }

    pub fn create_master_from_dialog(&mut self, cx: &mut Context<Self>) {
        let title = self.master_title_input.read(cx).text().trim().to_string();
        let final_title = if title.is_empty() {
            "Untitled Master CV".to_string()
        } else {
            title
        };

        let mut pii = self
            .active_document()
            .or_else(|| self.documents.first())
            .map(|d| d.pii_header.clone())
            .unwrap_or_default();
        if pii.full_name.trim().is_empty() {
            pii.full_name = "Your Name".to_string();
        }
        let body = crate::contracts::SanitizedCvBody {
            sections: vec![
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Professional summary".to_string(),
                    enabled: true,
                    kind: SectionKind::Summary {
                        text: String::new(),
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Professional experience".to_string(),
                    enabled: true,
                    kind: SectionKind::TimelineEntries {
                        entries: Vec::new(),
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Education".to_string(),
                    enabled: true,
                    kind: SectionKind::TimelineEntries {
                        entries: Vec::new(),
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Skills & others".to_string(),
                    enabled: true,
                    kind: SectionKind::CategorizedInlineList {
                        categories: Vec::new(),
                    },
                },
            ],
        };

        match self
            .services
            .repository
            .create_root(&final_title, pii, body)
        {
            Ok(doc) => {
                let doc_id = doc.id;
                self.documents.push(doc);
                self.active_doc_id = Some(doc_id);
                self.current_view = WorkspaceViewPage::Editor;
                self.active_dialog = None;
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);
                self.notify(format!("Master CV \"{final_title}\" created and saved."));
            }
            Err(err) => {
                self.notify_error(format!("Failed to create Master CV: {err}"));
            }
        }
    }

}