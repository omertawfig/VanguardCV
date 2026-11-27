use super::*;

impl WorkspaceView {
    // -----------------------------------------------------------------------
    // Default CV, Job Discovery & Factory Reset
    // -----------------------------------------------------------------------

    /// Marks or unmarks a CV as the workspace Default CV (persisted in SQLite).
    pub fn set_default_cv(&mut self, doc_id: DocumentId, cx: &mut Context<Self>) {
        if self.default_cv_id == Some(doc_id) {
            self.default_cv_id = None;
            let _ = self
                .services
                .repository
                .save_setting(settings_keys::DEFAULT_CV_ID, "");
            self.notify("Cleared default CV.");
            return;
        }
        let title = self
            .find_document(doc_id)
            .map(|d| d.title.clone())
            .unwrap_or_else(|| "CV".to_string());
        self.default_cv_id = Some(doc_id);
        let _ = self
            .services
            .repository
            .save_setting(settings_keys::DEFAULT_CV_ID, &doc_id.to_string());
        self.select_job_cv_source(CvSearchSource::Single(doc_id), cx);
        self.notify(format!(
            "Set \"{title}\" as default CV (used automatically on launch for Job discovery)."
        ));
    }

    pub(super) fn persist_job_profiles_cache(&self) {
        if let Some(json) = self.job_cache.serialize_persisted_profiles() {
            let _ = self
                .services
                .repository
                .save_setting(settings_keys::JOB_PROFILES_CACHE, &json);
        }
    }

    /// Runs silently at application startup: if a Default CV is set, prepares its
    /// saved keywords (auto-refreshing once via AI if configured and missing/stale,
    /// otherwise extracting locally) and fetches page 1 of last-24h opportunities.
    pub(super) fn bootstrap_default_cv_job_search(&mut self, cx: &mut Context<Self>) {
        let Some(def_id) = self.default_cv_id else {
            return;
        };
        if !self.documents.iter().any(|d| d.id == def_id) {
            return;
        }
        let source = CvSearchSource::Single(def_id);
        self.job_source = Some(source.clone());
        self.job_filters.posted_within_days = Some(1);
        self.job_filters.page = 1;

        let docs = self.resolved_job_source_docs(&source);
        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let hash = compute_cv_content_hash(&doc_refs);

        if let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &hash) {
            self.job_profiles = cached.to_vec();
            self.job_profiles_from_cache = true;
            self.job_profiles_used_fallback = used_fb;
            self.job_profiles_stale = false;
            self.search_jobs_internal(false, true, cx);
        } else {
            self.generate_job_profiles_internal(false, true, cx);
        }
    }

    /// Resolves the active CV source for Job Discovery, prioritizing explicit
    /// selection, then Default CV, then active document, then first Master CV.
    pub fn resolved_job_source(&self) -> Option<CvSearchSource> {
        if let Some(ref src) = self.job_source {
            match src {
                CvSearchSource::Single(id) if self.documents.iter().any(|d| d.id == *id) => {
                    return Some(src.clone());
                }
                CvSearchSource::AllCvs if !self.documents.is_empty() => {
                    return Some(CvSearchSource::AllCvs);
                }
                _ => {}
            }
        }
        if let Some(def_id) = self.default_cv_id
            && self.documents.iter().any(|d| d.id == def_id)
        {
            return Some(CvSearchSource::Single(def_id));
        }
        if let Some(active_id) = self.active_doc_id
            && self.documents.iter().any(|d| d.id == active_id)
        {
            return Some(CvSearchSource::Single(active_id));
        }
        self.documents
            .iter()
            .find(|d| d.parent_id.is_none())
            .or_else(|| self.documents.first())
            .map(|d| CvSearchSource::Single(d.id))
    }

    fn resolved_job_source_docs(&self, source: &CvSearchSource) -> Vec<CvDocument> {
        match source {
            CvSearchSource::Single(id) => self
                .documents
                .iter()
                .find(|d| d.id == *id)
                .cloned()
                .into_iter()
                .collect(),
            CvSearchSource::AllCvs => self.documents.clone(),
        }
    }

    /// Resolves which Master CV will serve as the parent when clicking "Tailor CV".
    pub fn resolved_job_target_master_id(&self) -> Option<DocumentId> {
        if let Some(CvSearchSource::Single(id)) = self.resolved_job_source()
            && let Some(doc) = self.find_document(id)
        {
            return Some(doc.parent_id.unwrap_or(doc.id));
        }
        if let Some(def_id) = self.default_cv_id
            && let Some(doc) = self.find_document(def_id)
        {
            return Some(doc.parent_id.unwrap_or(doc.id));
        }
        self.active_document()
            .map(|active| active.parent_id.unwrap_or(active.id))
            .or_else(|| {
                self.documents
                    .iter()
                    .find(|d| d.parent_id.is_none())
                    .map(|d| d.id)
            })
            .or_else(|| self.documents.first().map(|d| d.id))
    }

    pub fn resolved_job_target_master_title(&self) -> Option<String> {
        let id = self.resolved_job_target_master_id()?;
        self.find_document(id).map(|d| d.title.clone())
    }

    pub(super) fn sync_job_source_and_cached_profiles(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.resolved_job_source() else {
            return;
        };
        if self.job_source.as_ref() != Some(&source) {
            self.job_source = Some(source.clone());
        }
        let docs = self.resolved_job_source_docs(&source);
        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let hash = compute_cv_content_hash(&doc_refs);

        if let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &hash) {
            self.job_profiles_stale = false;
            if self.job_profiles.is_empty() {
                self.job_profiles = cached.to_vec();
                self.job_profiles_from_cache = true;
                self.job_profiles_used_fallback = used_fb;
                if self.job_listings.is_empty() && !self.job_listings_loading {
                    self.search_jobs_internal(false, true, cx);
                }
            }
        } else if self.job_cache.has_stale_profiles(&source, &hash) {
            // CV was edited since its keywords were last extracted — refresh lazily once!
            self.job_profiles_stale = true;
            if !self.job_profiles_loading {
                self.generate_job_profiles_internal(true, true, cx);
            }
        }
    }

    pub fn select_job_cv_source(&mut self, source: CvSearchSource, cx: &mut Context<Self>) {
        self.job_source = Some(source.clone());
        self.job_error = None;
        self.job_filters.page = 1;
        let docs = self.resolved_job_source_docs(&source);
        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let hash = compute_cv_content_hash(&doc_refs);
        if let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &hash) {
            self.job_profiles = cached.to_vec();
            self.job_profiles_from_cache = true;
            self.job_profiles_used_fallback = used_fb;
            self.job_profiles_stale = false;
            self.search_jobs(false, cx);
        } else if self.job_cache.has_stale_profiles(&source, &hash) {
            self.job_profiles.clear();
            self.job_profiles_from_cache = false;
            self.job_profiles_used_fallback = false;
            self.job_profiles_stale = true;
            self.generate_job_profiles_internal(true, false, cx);
        } else {
            self.job_profiles.clear();
            self.job_profiles_from_cache = false;
            self.job_profiles_used_fallback = false;
            self.job_profiles_stale = false;
        }
    }

    pub fn generate_job_profiles(&mut self, force_refresh: bool, cx: &mut Context<Self>) {
        self.generate_job_profiles_internal(force_refresh, false, cx);
    }

    fn generate_job_profiles_internal(
        &mut self,
        force_refresh: bool,
        silent: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.resolved_job_source() else {
            if !silent {
                self.notify("Create or select a CV first to generate search profiles.");
            }
            return;
        };
        self.job_source = Some(source.clone());
        self.job_filters.page = 1;
        let docs = self.resolved_job_source_docs(&source);
        if docs.is_empty() {
            if !silent {
                self.notify("No saved CVs found to analyze.");
            }
            return;
        }

        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let content_hash = compute_cv_content_hash(&doc_refs);

        if !force_refresh
            && let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &content_hash)
        {
            self.job_profiles = cached.to_vec();
            self.job_profiles_from_cache = true;
            self.job_profiles_used_fallback = used_fb;
            self.job_profiles_stale = false;
            self.job_error = None;
            self.search_jobs_internal(false, silent, cx);
            return;
        }

        if !self.is_ai_configured() {
            let local_profiles =
                extract_fallback_profiles(&doc_refs, self.services.privacy.as_ref());
            self.job_cache.put_profiles(
                &source,
                &content_hash,
                local_profiles.clone(),
                true,
            );
            self.persist_job_profiles_cache();
            self.job_profiles = local_profiles;
            self.job_profiles_from_cache = false;
            self.job_profiles_used_fallback = true;
            self.job_profiles_stale = false;
            self.job_error = None;
            if !silent {
                self.notify(
                    "Extracted and saved local search profiles for this CV. Connect AI in Settings for richer role suggestions.",
                );
            }
            self.search_jobs_internal(false, silent, cx);
            return;
        }

        self.job_profile_generation = self.job_profile_generation.wrapping_add(1);
        let generation = self.job_profile_generation;
        self.job_profiles_loading = true;
        self.job_error = None;

        let ai = self.services.ai.clone();
        let scrubber = self.services.privacy.clone();
        let source_for_task = source.clone();
        let hash_for_task = content_hash.clone();

        cx.spawn(async move |this, cx| {
            let handle = crate::runtime::handle().spawn(async move {
                let refs: Vec<&CvDocument> = docs.iter().collect();
                generate_search_profiles_with_ai(ai.as_ref(), &refs, scrubber.as_ref()).await
            });
            let result = match handle.await {
                Ok(res) => res,
                Err(join_err) => Err(anyhow::anyhow!("Profile generation task failed: {join_err}")),
            };

            let _ = this.update(cx, |view, cx| {
                if view.job_profile_generation != generation {
                    return;
                }
                view.job_profiles_loading = false;
                match result {
                    Ok((profiles, used_fallback)) => {
                        view.job_cache.put_profiles(
                            &source_for_task,
                            &hash_for_task,
                            profiles.clone(),
                            used_fallback,
                        );
                        view.persist_job_profiles_cache();
                        view.job_profiles = profiles;
                        view.job_profiles_from_cache = false;
                        view.job_profiles_used_fallback = used_fallback;
                        view.job_profiles_stale = false;
                        view.job_error = None;
                        if used_fallback && !silent {
                            view.notify(
                                "AI returned unstructured output; using clean local search profiles instead.",
                            );
                        }
                        view.search_jobs_internal(false, silent, cx);
                    }
                    Err(err) => {
                        let msg = format!("{err:#}");
                        view.job_error = Some(msg.clone());
                        if !silent || view.current_view == WorkspaceViewPage::Jobs {
                            if crate::ai::is_ai_limit_error(&msg) {
                                let lower = msg.to_ascii_lowercase();
                                if lower.starts_with("api") || lower.starts_with("usage limit") {
                                    view.notify_error(msg);
                                } else {
                                    view.notify_error(format!("API limit reached: {msg}"));
                                }
                            } else {
                                view.notify_error(format!(
                                    "Could not generate search profiles: {msg}"
                                ));
                            }
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Returns `true` when the active search profiles, toggled keywords, or
    /// custom query differ from the last executed job search so the UI can
    /// prompt the user to click "Update search".
    pub fn job_search_has_unapplied_changes(&self) -> bool {
        if self.job_listings_loading || self.job_profiles_loading {
            return false;
        }
        let has_query = !self.job_filters.manual_query.trim().is_empty()
            || self.job_profiles.iter().any(|p| p.enabled);
        if !has_query {
            return false;
        }
        let current_sig = self.job_filters.cache_signature(&self.job_profiles);
        self.job_last_searched_signature.as_deref() != Some(current_sig.as_str())
    }

    fn persist_current_profile_preferences(&mut self) {
        if let Some(source) = self.resolved_job_source() {
            let docs = self.resolved_job_source_docs(&source);
            let doc_refs: Vec<&CvDocument> = docs.iter().collect();
            let hash = compute_cv_content_hash(&doc_refs);
            self.job_cache.put_profiles(
                &source,
                &hash,
                self.job_profiles.clone(),
                self.job_profiles_used_fallback,
            );
            self.persist_job_profiles_cache();
        }
    }

    pub fn toggle_job_profile(&mut self, profile_id: &str, _cx: &mut Context<Self>) {
        if let Some(p) = self.job_profiles.iter_mut().find(|p| p.id == profile_id) {
            p.enabled = !p.enabled;
        }
        // Persist toggled profile preference for this CV without burning AI
        // tokens or auto-firing an upstream search while the user is choosing roles.
        self.persist_current_profile_preferences();
        self.job_filters.page = 1;
    }

    pub fn toggle_job_profile_keyword(
        &mut self,
        profile_id: &str,
        keyword: &str,
        _cx: &mut Context<Self>,
    ) {
        if let Some(p) = self.job_profiles.iter_mut().find(|p| p.id == profile_id)
            && !p.disabled_keywords.remove(keyword)
        {
            p.disabled_keywords.insert(keyword.to_string());
        }
        // Persist toggled keyword preference for this CV without burning AI
        // tokens or auto-firing an upstream search while the user is tuning keywords.
        self.persist_current_profile_preferences();
        self.job_filters.page = 1;
    }

    pub fn toggle_job_remote_filter(&mut self, cx: &mut Context<Self>) {
        self.job_filters.remote_only = !self.job_filters.remote_only;
        self.job_filters.page = 1;
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        }
    }

    pub fn toggle_job_country_dropdown(&mut self) {
        self.job_country_dropdown_open = !self.job_country_dropdown_open;
    }

    pub fn close_job_country_dropdown(&mut self) {
        self.job_country_dropdown_open = false;
    }

    pub fn set_job_country(&mut self, country_code: Option<String>, cx: &mut Context<Self>) {
        let normalized = country_code.and_then(|raw| normalize_country_code(&raw));
        self.job_country_dropdown_open = false;
        if self.job_filters.country == normalized {
            return;
        }
        self.job_filters.country = normalized.clone();
        self.job_filters.page = 1;
        let _ = self.services.repository.save_setting(
            settings_keys::JOB_COUNTRY,
            normalized.as_deref().unwrap_or(""),
        );
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        }
    }

    pub fn job_country_base_scroll(&self) -> ScrollHandle {
        self.job_country_scroll.0.borrow().base_handle.clone()
    }

    pub fn scroll_job_country_by(&mut self, dy: Pixels, cx: &mut Context<Self>) -> bool {
        let handle = self.job_country_base_scroll();
        let cur = handle.offset();
        let max_y = handle.max_offset().height;
        if max_y <= px(0.5) {
            return false;
        }
        let next_y = (cur.y + dy).clamp(-max_y, px(0.0));
        if (next_y - cur.y).abs() > px(0.1) {
            handle.set_offset(point(cur.x, next_y));
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn set_job_freshness_days(&mut self, days: Option<u32>, cx: &mut Context<Self>) {
        if self.job_filters.posted_within_days == days {
            return;
        }
        self.job_filters.posted_within_days = days;
        self.job_filters.page = 1;
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        }
    }

    pub fn set_job_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let clamped = page.clamp(1, MAX_PAGE_NUMBER);
        if self.job_filters.page == clamped {
            return;
        }
        self.job_filters.page = clamped;
        self.search_jobs(false, cx);
    }

    pub fn set_job_page_size(&mut self, size: usize, cx: &mut Context<Self>) {
        let clamped = size.clamp(5, MAX_PAGE_SIZE);
        if self.job_filters.page_size == clamped {
            return;
        }
        self.job_filters.page_size = clamped;
        self.job_filters.page = 1;
        self.search_jobs(false, cx);
    }

    pub fn clear_job_manual_query(&mut self, cx: &mut Context<Self>) {
        self.job_filters.manual_query.clear();
        self.job_filters.page = 1;
        self.job_query_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
        if self.job_profiles.iter().any(|p| p.enabled) {
            self.search_jobs(false, cx);
        } else {
            if let Some((_, abort_handle)) = self.active_job_search_task.take() {
                abort_handle.abort();
            }
            self.job_search_generation = self.job_search_generation.wrapping_add(1);
            self.job_listings_loading = false;
            self.job_listings.clear();
            self.job_total_estimated = 0;
            self.job_has_next_page = false;
            self.job_listings_from_cache = false;
            self.job_last_searched_signature = None;
        }
    }

    pub fn search_jobs(&mut self, force_refresh: bool, cx: &mut Context<Self>) {
        self.search_jobs_internal(force_refresh, false, cx);
    }

    fn search_jobs_internal(&mut self, force_refresh: bool, silent: bool, cx: &mut Context<Self>) {
        self.job_filters.manual_query = self.job_query_input.read(cx).text().to_string();
        let has_query = !self.job_filters.manual_query.trim().is_empty()
            || self.job_profiles.iter().any(|p| p.enabled);
        if !has_query {
            self.job_listings.clear();
            self.job_total_estimated = 0;
            self.job_has_next_page = false;
            self.job_listings_from_cache = false;
            self.job_last_searched_signature = None;
            return;
        }

        let cache_sig = self.job_filters.cache_signature(&self.job_profiles);
        if force_refresh {
            if self.job_cache.cooldown_remaining().is_some() {
                if !silent {
                    self.notify("Please wait a moment before refreshing again.");
                }
                return;
            }
            self.job_cache.invalidate_listings();
        } else {
            if let Some(cached) = self.job_cache.get_listings(&cache_sig) {
                if let Some((_, abort_handle)) = self.active_job_search_task.take() {
                    abort_handle.abort();
                }
                self.job_listings_loading = false;
                self.job_listings = cached.jobs;
                self.job_total_estimated = cached.total_estimated;
                self.job_has_next_page = cached.has_next_page;
                self.job_warnings = cached.warnings;
                self.job_listings_from_cache = true;
                self.job_last_searched_signature = Some(cache_sig);
                self.job_error = None;
                return;
            }
            // Deduplicate: if the exact same search is already in flight, let it finish
            // instead of firing parallel requests that trigger upstream timeouts / 429s.
            if self.job_listings_loading
                && self
                    .active_job_search_task
                    .as_ref()
                    .is_some_and(|(sig, _)| sig == &cache_sig)
            {
                return;
            }
        }

        if let Some((_, abort_handle)) = self.active_job_search_task.take() {
            abort_handle.abort();
        }

        self.job_cache.record_live_fetch();
        self.job_search_generation = self.job_search_generation.wrapping_add(1);
        let generation = self.job_search_generation;
        self.job_listings_loading = true;
        self.job_error = None;

        let provider = self.job_provider.clone();
        let profiles = self.job_profiles.clone();
        let filters = self.job_filters.clone();

        let join_handle = crate::runtime::handle()
            .spawn(async move { provider.search(&profiles, &filters).await });
        self.active_job_search_task = Some((cache_sig.clone(), join_handle.abort_handle()));

        cx.spawn(async move |this, cx| {
            let result = match join_handle.await {
                Ok(res) => res,
                Err(join_err) if join_err.is_cancelled() => return,
                Err(join_err) => Err(anyhow::anyhow!("Job search task failed: {join_err}")),
            };

            let _ = this.update(cx, |view, cx| {
                if view.job_search_generation != generation {
                    return;
                }
                view.active_job_search_task = None;
                view.job_listings_loading = false;
                match result {
                    Ok(search_res) => {
                        view.job_cache.put_listings(&cache_sig, search_res.clone());
                        view.job_listings = search_res.jobs;
                        view.job_total_estimated = search_res.total_estimated;
                        view.job_has_next_page = search_res.has_next_page;
                        view.job_warnings = search_res.warnings;
                        view.job_listings_from_cache = false;
                        view.job_last_searched_signature = Some(cache_sig);
                        view.job_expanded_slugs.clear();
                        view.job_error = None;
                    }
                    Err(err) => {
                        // Allow immediate manual Retry after a failed search.
                        view.job_cache.clear_cooldown();
                        let msg = format!("{err:#}");
                        view.job_error = Some(msg.clone());
                        if !silent || view.current_view == WorkspaceViewPage::Jobs {
                            view.notify_error(msg);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn toggle_job_jd_expanded(&mut self, slug: &str) {
        if !self.job_expanded_slugs.remove(slug) {
            self.job_expanded_slugs.insert(slug.to_string());
        }
    }

    pub fn dismiss_job_error(&mut self) {
        self.job_error = None;
    }

    pub fn open_job_url(&mut self, url: &str) {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return;
        }
        self.open_url_or_notify(trimmed, "job link");
    }

    /// One-click closed-loop handoff: branches a new variation from the chosen
    /// Master CV pre-populated with the job's company, role, full Markdown JD,
    /// and employer URL, opens the Editor, and starts AI tailoring automatically.
    pub fn tailor_cv_for_job(&mut self, job: JobListing, cx: &mut Context<Self>) {
        if self.ai_state.is_running {
            self.notify("Wait for the current AI tailoring run to finish before starting another.");
            return;
        }
        let Some(root_id) = self.resolved_job_target_master_id() else {
            self.notify("Create a Master CV first before tailoring for a job.");
            return;
        };

        self.flush_pending_now();

        let company = job.company.trim().to_string();
        let role = job.title.trim().to_string();
        let title = if !company.is_empty() && !role.is_empty() {
            format!("{company} — {role}")
        } else if !role.is_empty() {
            role.clone()
        } else if !company.is_empty() {
            format!("{company} — Variation")
        } else {
            "Tailored Variation".to_string()
        };

        let mut applied_skill_ids: Vec<String> = self
            .services
            .skills
            .list()
            .into_iter()
            .filter(|s| s.enabled && s.default_enabled)
            .map(|s| s.id)
            .collect();
        if applied_skill_ids.is_empty() {
            applied_skill_ids = self
                .services
                .skills
                .list()
                .into_iter()
                .filter(|s| s.enabled)
                .map(|s| s.id)
                .collect();
        }

        let jd = if !job.description_markdown.trim().is_empty() {
            job.description_markdown.trim().to_string()
        } else {
            job.preview.trim().to_string()
        };

        let job_ctx = JobContext {
            company_name: company,
            target_role: role,
            job_description_raw: jd,
            application_url: (!job.apply_url.trim().is_empty()).then(|| job.apply_url.clone()),
            applied_skill_ids,
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

                if self.services.ai.is_configured() {
                    self.start_ai_tailoring_for_active(cx);
                } else {
                    self.notify(format!(
                        "Branched \"{title}\" with full job description. Connect ChatGPT or Google Gemini in Settings to run automated tailoring."
                    ));
                }
            }
            Err(err) => {
                self.notify_error(format!("Failed to branch CV variation for job: {err}"));
            }
        }
    }

    pub fn open_reset_app_dialog(&mut self) {
        self.active_dialog = Some(Dialog::ConfirmResetApp);
    }

    /// Factory-resets the entire application: wipes SQLite (all CVs, variations,
    /// snapshots, saved job profiles, settings — keeping `workspace.seeded=true`
    /// so the library stays empty), deletes custom user skills & restores bundled
    /// default skills, and clears saved ChatGPT & Google Gemini credentials.
    pub fn reset_application_all(&mut self, cx: &mut Context<Self>) {
        // Cancel any pending debounced saves so they cannot write back after reset.
        self.debounced_saver.discard();
        self.save_generation = self.save_generation.wrapping_add(1);
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
        self.job_profile_generation = self.job_profile_generation.wrapping_add(1);
        self.job_search_generation = self.job_search_generation.wrapping_add(1);

        if let Err(err) = self.services.repository.reset_all() {
            self.active_dialog = None;
            self.notify_error(format!("Database reset failed: {err}"));
            return;
        }

        if let Err(err) = self.services.skills.reset_to_defaults() {
            tracing::warn!("Skill registry reset reported warning: {err}");
        }

        // Clear OS Credential Manager entries & reset AI provider state.
        let _ = credentials::clear_gemini_api_key();
        let ai_for_signout = self.services.ai.clone();
        crate::runtime::handle().spawn(async move {
            let _ = ai_for_signout.sign_out().await;
        });
        let _ = self.services.ai.configure(AiConfig::default());

        // Clear all in-memory workspace state.
        self.documents.clear();
        self.active_doc_id = None;
        self.default_cv_id = None;
        self.proposals.clear();
        self.ats_scores.clear();
        self.ai_summaries.clear();
        self.snapshots.clear();
        self.ai_state = AiAgentState::default();
        self.rendered_images.clear();
        self.preview_error = None;
        self.is_rendering_pdf = false;
        self.is_exporting_pdf = false;
        self.editing_proposal_id = None;
        self.active_dialog = None;

        // Reset AI UI fields.
        self.active_provider = AiProviderKind::ChatGpt;
        self.chatgpt_auth = ChatGptAuthStatus::default();
        self.auth_in_progress = false;
        self.auth_error = None;
        self.show_plan_welcome_banner = false;
        self.gemini_key_configured = false;
        self.gemini_key_visible = false;
        self.selected_model = DEFAULT_CHATGPT_MODEL.to_string();
        self.selected_gemini_model = DEFAULT_GEMINI_MODEL.to_string();
        self.available_models.clear();
        self.models_loading = false;
        self.models_error = None;
        self.model_filter_query.clear();

        // Reset Job Discovery state & caches.
        self.job_source = None;
        self.job_profiles.clear();
        self.job_profiles_from_cache = false;
        self.job_profiles_used_fallback = false;
        self.job_profiles_stale = false;
        self.job_filters = JobFilters::default();
        self.job_country_dropdown_open = false;
        self.job_country_base_scroll().set_offset(point(px(0.0), px(0.0)));
        if let Some((_, abort_handle)) = self.active_job_search_task.take() {
            abort_handle.abort();
        }
        self.job_listings.clear();
        self.job_total_estimated = 0;
        self.job_has_next_page = false;
        self.job_listings_from_cache = false;
        self.job_last_searched_signature = None;
        self.job_warnings.clear();
        self.job_expanded_slugs.clear();
        self.job_profiles_loading = false;
        self.job_listings_loading = false;
        self.job_error = None;
        self.job_cache.clear_all();

        // Reset auto-update state to default enabled.
        self.update_generation = self.update_generation.wrapping_add(1);
        self.auto_update_enabled = true;
        self.update_status = UpdateStatus::Idle;
        self.update_release = None;
        self.update_downloaded = None;
        self.update_error = None;
        self.import_draft = None;
        self.import_loading = false;

        // Reset theme & search inputs.
        self.library_query.clear();
        self.set_theme_mode(ThemeMode::Dark);
        self.gemini_api_key_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.search_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.model_filter_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.job_query_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        self.current_view = WorkspaceViewPage::Library;
        self.notify(
            "Workspace reset complete. All CVs, settings, and AI credentials were cleared; default skills preserved.",
        );
    }

}