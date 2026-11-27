use super::*;
use crate::platform::{FileDialogFilter, OpenFileRequest, SaveFileRequest};

impl WorkspaceView {
    // -----------------------------------------------------------------------
    // AI Skills Management (Real FileSkillRegistry CRUD)
    // -----------------------------------------------------------------------

    pub fn open_add_skill_dialog(&mut self, cx: &mut Context<Self>) {
        self.editing_skill_id = None;
        self.skill_name_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("");
            cx.notify();
        });
        self.skill_desc_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("");
            cx.notify();
        });
        self.skill_category_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("Custom");
            cx.notify();
        });
        self.skill_inst_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSkill {
            is_new: true,
            read_only: false,
        });
    }

    pub fn open_edit_skill_dialog(&mut self, skill: &SkillManifest, cx: &mut Context<Self>) {
        let is_user = self.is_user_owned_skill(skill);
        let read_only = !is_user;
        self.editing_skill_id = Some(skill.id.clone());
        self.skill_name_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.name.clone());
            cx.notify();
        });
        self.skill_desc_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.description.clone().unwrap_or_default());
            cx.notify();
        });
        self.skill_category_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.category.clone());
            cx.notify();
        });
        self.skill_inst_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.prompt_injection.system_directives.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSkill {
            is_new: false,
            read_only,
        });
    }

    pub fn save_skill_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (is_new, read_only) = match self.active_dialog {
            Some(Dialog::EditSkill { is_new, read_only }) => (is_new, read_only),
            _ => return,
        };
        if read_only {
            self.active_dialog = None;
            return;
        }

        let name = self.skill_name_input.read(cx).text().trim().to_string();
        let desc = self.skill_desc_input.read(cx).text().trim().to_string();
        let category = self.skill_category_input.read(cx).text().trim().to_string();
        let instructions = self.skill_inst_input.read(cx).text().trim().to_string();

        if name.is_empty() || instructions.is_empty() {
            self.notify_error("Skill name and instructions are required.");
            return;
        }

        let slug = if is_new {
            slugify_skill_id(&name)
        } else {
            self.editing_skill_id
                .clone()
                .unwrap_or_else(|| slugify_skill_id(&name))
        };
        if slug.is_empty() {
            self.notify_error("Skill name must contain letters or digits.");
            return;
        }

        let final_cat = if category.is_empty() {
            "Custom".to_string()
        } else {
            category
        };
        let final_desc = if desc.is_empty() { name.clone() } else { desc };

        let content = format!(
            "---\nname: \"{}\"\ndescription: \"{}\"\ncategory: \"{}\"\nversion: \"1.0.0\"\n---\n\n{}\n",
            escape_yaml_str(&name),
            escape_yaml_str(&final_desc),
            escape_yaml_str(&final_cat),
            instructions
        );

        let skills = self.services.skills.clone();
        let skills_dir = skills.skills_dir().to_path_buf();
        let dir = skills_dir.join(&slug);
        let slug_clone = slug.clone();
        let name_clone = name.clone();

        cx.spawn(async move |this, cx| {
            let write_result = cx
                .background_executor()
                .spawn(async move {
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(dir.join("SKILL.md"), content)?;
                    // Remove legacy flat .toml / .md duplicate in user skills dir if converting from imported file
                    for ext in ["toml", "md"] {
                        let flat = skills_dir.join(format!("{slug_clone}.{ext}"));
                        if flat.is_file() {
                            let _ = std::fs::remove_file(flat);
                        }
                    }
                    Ok::<(), std::io::Error>(())
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                if let Err(err) = write_result {
                    view.notify_error(format!("Failed to write SKILL.md: {err}"));
                    cx.notify();
                    return;
                }
                match skills.reload() {
                    Ok(_) => {
                        view.active_dialog = None;
                        view.notify(if is_new {
                            format!("Skill \"{name_clone}\" saved to user skills directory.")
                        } else {
                            format!("Skill \"{name_clone}\" updated.")
                        });
                    }
                    Err(err) => {
                        view.notify_error(format!("Skill saved, but reload failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn toggle_skill_enabled(&mut self, skill_id: &str) {
        let (current, display_name) = self
            .services
            .skills
            .get(skill_id)
            .map(|s| (s.enabled, s.name))
            .unwrap_or_else(|| (false, skill_id.to_string()));
        match self.services.skills.set_skill_enabled(skill_id, !current) {
            Ok(()) => {
                self.notify(if !current {
                    format!("Enabled \"{display_name}\".")
                } else {
                    format!("Disabled \"{display_name}\".")
                });
            }
            Err(err) => {
                self.notify_error(format!("Could not toggle skill: {err}"));
            }
        }
    }

    pub fn delete_skill(&mut self, skill_id: &str) {
        let display_name = self
            .services
            .skills
            .get(skill_id)
            .map(|s| s.name)
            .unwrap_or_else(|| skill_id.to_string());
        match self.services.skills.delete_skill(skill_id) {
            Ok(()) => {
                self.variation_skills.remove(skill_id);
                self.notify(format!("Deleted skill \"{display_name}\"."));
            }
            Err(err) => {
                self.notify_error(format!("Could not delete skill: {err}"));
            }
        }
    }

    pub fn restore_default_skills(&mut self) {
        match self.services.skills.restore_bundled_skills() {
            Ok(_) => {
                self.notify("Restored default bundled AI skills.");
            }
            Err(err) => {
                self.notify_error(format!("Could not restore default skills: {err}"));
            }
        }
    }

    pub fn reload_skills(&mut self) {
        match self.services.skills.reload() {
            Ok(list) => {
                self.notify(format!("Reloaded {} AI skills.", list.len()));
            }
            Err(err) => {
                self.notify_error(format!("Skill reload failed: {err}"));
            }
        }
    }

    pub fn import_skill_from_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let skills = self.services.skills.clone();
        let path_rx = Self::prompt_open_single_file(
            OpenFileRequest {
                title: "Import AI skill (.md or .toml)".to_string(),
                initial_dir: Some(Self::initial_save_dir()),
                filters: vec![FileDialogFilter {
                    label: "AI Skill Files (*.md, *.toml)",
                    extensions: &["md", "toml"],
                }],
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
            let result = cx
                .background_executor()
                .spawn(async move { skills.import_skill(&path) })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(manifest) => {
                        view.notify(format!(
                            "Imported skill \"{}\" ({}).",
                            manifest.name, manifest.id
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Skill import failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -----------------------------------------------------------------------
    // Settings, Sign in with ChatGPT, Backups, JSON Import/Export & Snapshots
    // -----------------------------------------------------------------------

    pub fn open_chatgpt_usage_settings(&mut self) {
        self.open_url_or_notify(CHATGPT_USAGE_SETTINGS_URL, "ChatGPT usage settings");
    }

    pub fn dismiss_plan_welcome_banner(&mut self) {
        self.show_plan_welcome_banner = false;
        let _ = self
            .services
            .repository
            .save_setting("ui.chatgpt_welcome_seen", "true");
    }

    pub fn start_chatgpt_sign_in(
        &mut self,
        force_consent: bool,
        fresh_registration: bool,
        cx: &mut Context<Self>,
    ) {
        if self.auth_in_progress {
            self.notify("A Sign in with ChatGPT flow is already waiting in your browser.");
            return;
        }

        self.auth_in_progress = true;
        self.auth_error = None;
        self.notify("Opening ChatGPT sign-in in your browser…");

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<ChatGptAuthStatus>>();

        crate::runtime::handle().spawn(async move {
            let result = ai
                .sign_in_with_chatgpt(force_consent, fresh_registration)
                .await;
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            if let Some(result) = rx.recv().await {
                let _ = this.update(cx, |view, cx| {
                    view.auth_in_progress = false;
                    match result {
                        Ok(status) => {
                            view.chatgpt_auth = status.clone();
                            view.auth_error = None;
                            if status.is_ready_for_inference() {
                                if view.active_provider != AiProviderKind::ChatGpt {
                                    view.active_provider = AiProviderKind::ChatGpt;
                                    let mut cfg = view.services.ai.config();
                                    cfg.provider = AiProviderKind::ChatGpt;
                                    let _ = view.services.ai.configure(cfg);
                                    let _ = view
                                        .services
                                        .repository
                                        .save_setting("ai.provider", AiProviderKind::ChatGpt.as_str());
                                }
                                let welcome_seen = view
                                    .services
                                    .repository
                                    .load_setting("ui.chatgpt_welcome_seen")
                                    .ok()
                                    .flatten()
                                    .is_some_and(|v| v == "true");
                                if !welcome_seen {
                                    view.show_plan_welcome_banner = true;
                                }
                                view.notify(format!(
                                    "Connected to ChatGPT as {}. Loading available models…",
                                    status.account_label()
                                ));
                                view.fetch_available_models(false, cx);
                            } else {
                                view.available_models.clear();
                                view.notify(
                                    "Signed in to ChatGPT, but ChatGPT plan usage was not enabled. Click \"Enable plan usage\" to grant access.",
                                );
                            }
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            view.chatgpt_auth = view.services.ai.auth_status();
                            view.auth_error = Some(msg.clone());
                            view.notify_error(msg);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn sign_out_chatgpt(&mut self, cx: &mut Context<Self>) {
        if self.auth_in_progress {
            return;
        }
        self.auth_in_progress = true;
        self.auth_error = None;

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<Option<String>>>();

        crate::runtime::handle().spawn(async move {
            let result = ai.sign_out().await;
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            if let Some(result) = rx.recv().await {
                let _ = this.update(cx, |view, cx| {
                    view.auth_in_progress = false;
                    view.chatgpt_auth = view.services.ai.auth_status();
                    view.available_models.clear();
                    view.models_error = None;
                    view.models_loading = false;
                    view.show_plan_welcome_banner = false;
                    match result {
                        Ok(Some(warning)) => view.notify(warning),
                        Ok(None) => view.notify("Signed out of ChatGPT and revoked local session."),
                        Err(err) => view.notify_error(format!("Sign-out error: {err}")),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn set_ai_provider(&mut self, provider: AiProviderKind, cx: &mut Context<Self>) {
        if self.active_provider == provider {
            return;
        }
        self.active_provider = provider;
        self.available_models.clear();
        self.models_error = None;
        self.models_loading = false;
        self.model_filter_query.clear();
        self.reset_models_list_scroll();
        self.model_filter_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        let mut cfg = self.services.ai.config();
        cfg.provider = provider;
        if cfg.base_url.trim().is_empty() {
            cfg.base_url = CHATGPT_API_BASE_URL.to_string();
        }
        if cfg.gemini_base_url.trim().is_empty() {
            cfg.gemini_base_url = GEMINI_API_BASE_URL.to_string();
        }
        if cfg.gemini_model.trim().is_empty() {
            cfg.gemini_model = DEFAULT_GEMINI_MODEL.to_string();
        }
        if let Err(err) = self.services.ai.configure(cfg) {
            self.notify_error(format!("Failed to switch AI provider: {err}"));
            return;
        }
        let _ = self
            .services
            .repository
            .save_setting("ai.provider", provider.as_str());

        self.notify(format!("Switched AI engine to {}.", provider.display_name()));
        if self.is_ai_configured() {
            self.fetch_available_models(false, cx);
        }
    }

    pub fn open_gemini_api_key_page(&mut self) {
        self.open_url_or_notify(GEMINI_API_KEY_URL, "Google AI Studio");
    }

    pub fn toggle_gemini_key_visibility(&mut self, cx: &mut Context<Self>) {
        self.gemini_key_visible = !self.gemini_key_visible;
        let masked = !self.gemini_key_visible;
        self.gemini_api_key_input.update(cx, |i, cx| {
            i.set_masked(masked, cx);
        });
    }

    pub fn save_gemini_api_key(&mut self, cx: &mut Context<Self>) {
        let key = self
            .gemini_api_key_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        if key.is_empty() {
            self.notify_error("Paste a Google Gemini API key first.");
            return;
        }

        if let Err(err) = credentials::save_gemini_api_key(&key) {
            self.notify_error(format!("Could not save Gemini API key: {err}"));
            return;
        }

        self.gemini_key_configured = true;
        let mut cfg = self.services.ai.config();
        cfg.provider = AiProviderKind::GeminiApi;
        cfg.gemini_api_key = Some(key);
        if cfg.gemini_base_url.trim().is_empty() {
            cfg.gemini_base_url = GEMINI_API_BASE_URL.to_string();
        }
        if cfg.gemini_model.trim().is_empty() {
            cfg.gemini_model = self.selected_gemini_model.clone();
        }
        if let Err(err) = self.services.ai.configure(cfg) {
            self.notify_error(format!("Failed to configure Gemini API: {err}"));
            return;
        }
        self.active_provider = AiProviderKind::GeminiApi;
        let _ = self
            .services
            .repository
            .save_setting("ai.provider", AiProviderKind::GeminiApi.as_str());

        self.notify("Gemini API key saved locally. Loading available Gemini models…");
        self.fetch_available_models(true, cx);
    }

    pub fn clear_gemini_api_key(&mut self, cx: &mut Context<Self>) {
        let _ = credentials::clear_gemini_api_key();
        self.gemini_key_configured = false;
        self.gemini_api_key_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        let mut cfg = self.services.ai.config();
        cfg.gemini_api_key = None;
        let _ = self.services.ai.configure(cfg);
        if self.active_provider == AiProviderKind::GeminiApi {
            self.available_models.clear();
            self.models_error = None;
            self.models_loading = false;
        }
        self.notify("Removed Google Gemini API key.");
    }

    pub fn active_model_display_name(&self) -> String {
        let current = self.active_model_id();
        self.available_models
            .iter()
            .find(|m| m.id == current)
            .map(|m| m.display_name.clone())
            .unwrap_or_else(|| current.to_string())
    }

    pub fn clear_model_filter(&mut self, cx: &mut Context<Self>) {
        self.model_filter_query.clear();
        self.reset_models_list_scroll();
        self.model_filter_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
    }

    pub fn filtered_available_models(&self) -> Vec<&ModelInfo> {
        let query = self.model_filter_query.trim().to_lowercase();
        if query.is_empty() {
            return self.available_models.iter().collect();
        }
        self.available_models
            .iter()
            .filter(|m| {
                m.id.to_lowercase().contains(&query)
                    || m.display_name.to_lowercase().contains(&query)
                    || m.description.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn select_model(&mut self, model_id: &str, notify_user: bool, _cx: &mut Context<Self>) {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return;
        }
        let mut cfg = self.services.ai.config();
        let setting_key = match self.active_provider {
            AiProviderKind::ChatGpt => {
                self.selected_model = trimmed.to_string();
                if cfg.base_url.trim().is_empty() {
                    cfg.base_url = CHATGPT_API_BASE_URL.to_string();
                }
                cfg.model = self.selected_model.clone();
                "ai.model"
            }
            AiProviderKind::GeminiApi => {
                self.selected_gemini_model = trimmed.to_string();
                if cfg.gemini_base_url.trim().is_empty() {
                    cfg.gemini_base_url = GEMINI_API_BASE_URL.to_string();
                }
                cfg.gemini_model = self.selected_gemini_model.clone();
                "ai.gemini_model"
            }
        };
        if let Err(err) = self.services.ai.configure(cfg) {
            self.notify_error(format!("Failed to select model: {err}"));
            return;
        }
        let _ = self
            .services
            .repository
            .save_setting(setting_key, trimmed);
        if notify_user {
            let label = self.active_model_display_name();
            self.notify(format!(
                "Selected {} model: {label}",
                self.active_provider_name()
            ));
        }
    }

    pub fn fetch_available_models(&mut self, notify_on_success: bool, cx: &mut Context<Self>) {
        self.chatgpt_auth = self.services.ai.auth_status();
        let provider = self.active_provider;

        match provider {
            AiProviderKind::ChatGpt => {
                if !self.chatgpt_auth.is_ready_for_inference() {
                    self.models_loading = false;
                    self.available_models.clear();
                    self.models_error = if self.chatgpt_auth.signed_in
                        && !self.chatgpt_auth.plan_usage_enabled
                    {
                        Some("Enable ChatGPT plan usage above to load available models.".to_string())
                    } else {
                        Some("Sign in with ChatGPT above to browse models included in your plan.".to_string())
                    };
                    if notify_on_success {
                        self.notify_error("Sign in with ChatGPT first to load models.");
                    }
                    return;
                }
            }
            AiProviderKind::GeminiApi => {
                if !self.gemini_key_configured {
                    self.models_loading = false;
                    self.available_models.clear();
                    self.models_error = Some(
                        "Save your Google Gemini API key above to load available Gemini models."
                            .to_string(),
                    );
                    if notify_on_success {
                        self.notify_error("Save a Google Gemini API key first to load models.");
                    }
                    return;
                }
            }
        }

        self.models_fetch_generation = self.models_fetch_generation.wrapping_add(1);
        let generation = self.models_fetch_generation;
        self.models_loading = true;
        self.models_error = None;

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<Vec<ModelInfo>>>();

        crate::runtime::handle().spawn(async move {
            let result = ai.list_models().await;
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            if let Some(result) = rx.recv().await {
                let _ = this.update(cx, |view, cx| {
                    if view.models_fetch_generation != generation
                        || view.active_provider != provider
                    {
                        return;
                    }
                    view.models_loading = false;
                    view.chatgpt_auth = view.services.ai.auth_status();
                    let provider_label = provider.display_name();
                    match result {
                        Ok(models) => {
                            view.models_error = None;
                            let count = models.len();
                            let active_id = view.active_model_id().to_string();
                            let current_valid = models.iter().any(|m| m.id == active_id);
                            let auto_pick = if !current_valid && !models.is_empty() {
                                models.first().map(|m| m.id.clone())
                            } else {
                                None
                            };
                            view.available_models = models;
                            view.reset_models_list_scroll();
                            if let Some(chosen) = auto_pick {
                                view.select_model(&chosen, false, cx);
                            }
                            if notify_on_success {
                                view.notify(format!(
                                    "Loaded {count} available models from {provider_label}."
                                ));
                            }
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            view.models_error = Some(msg.clone());
                            if notify_on_success {
                                view.notify_error(format!(
                                    "Could not fetch models from {provider_label}: {msg}"
                                ));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn backup_database(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let default_name = format!("vanguardcv-backup-{}.db", Utc::now().format("%Y%m%d-%H%M"));
        let initial_dir = Self::initial_save_dir();
        let path_rx = Self::prompt_save_single_file(
            SaveFileRequest {
                title: "Save Database Backup".to_string(),
                initial_dir: Some(initial_dir),
                default_name: Some(default_name),
                default_extension: Some("db".to_string()),
                filters: vec![FileDialogFilter {
                    label: "SQLite Database (*.db)",
                    extensions: &["db"],
                }],
            },
            cx,
        );

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("db");
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    repo.backup_to(&path)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!("Database backed up to {}", path.display()));
                    }
                    Err(err) => {
                        view.notify_error(format!("Database backup failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn export_active_json(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active_document() else {
            self.notify("Select a CV from the library before exporting JSON.");
            return;
        };
        let active_id = active.id;
        let slug = slugify_skill_id(&active.title);
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let default_name = format!("{}.json", if slug.is_empty() { "cv" } else { &slug });
        let initial_dir = Self::initial_save_dir();
        let path_rx = Self::prompt_save_single_file(
            SaveFileRequest {
                title: "Export CV as JSON".to_string(),
                initial_dir: Some(initial_dir),
                default_name: Some(default_name),
                default_extension: Some("json".to_string()),
                filters: vec![FileDialogFilter {
                    label: "JSON Files (*.json)",
                    extensions: &["json"],
                }],
            },
            cx,
        );

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("json");
            }

            let result = cx
                .background_executor()
                .spawn(async move {
                    let json = repo.export_json(active_id)?;
                    std::fs::write(&path, json)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!("Exported CV JSON to {}", path.display()));
                    }
                    Err(err) => {
                        view.notify_error(format!("JSON export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn import_cv_json(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let path_rx = Self::prompt_open_single_file(
            OpenFileRequest {
                title: "Import CV from JSON".to_string(),
                initial_dir: Some(Self::initial_save_dir()),
                filters: vec![FileDialogFilter {
                    label: "JSON Files (*.json)",
                    extensions: &["json"],
                }],
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
            let result = cx
                .background_executor()
                .spawn(async move {
                    let raw = std::fs::read_to_string(&path)?;
                    repo.import_json(&raw)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(doc) => {
                        let title = doc.title.clone();
                        let id = doc.id;
                        view.documents.push(doc);
                        view.active_doc_id = Some(id);
                        view.refresh_snapshots();
                        view.trigger_pdf_preview_render(cx);
                        view.notify(format!("Imported \"{title}\" as a new Master CV."));
                    }
                    Err(err) => {
                        view.notify_error(format!("JSON import failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn export_vanguard_archive(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let skills = self.services.skills.clone();
        let default_name = format!(
            "vanguardcv-workspace-{}.vanguard",
            Utc::now().format("%Y%m%d-%H%M")
        );
        let initial_dir = Self::initial_save_dir();
        let path_rx = Self::prompt_save_single_file(
            SaveFileRequest {
                title: "Export VanguardCV Workspace Archive".to_string(),
                initial_dir: Some(initial_dir),
                default_name: Some(default_name),
                default_extension: Some("vanguard".to_string()),
                filters: vec![FileDialogFilter {
                    label: "VanguardCV Archive (*.vanguard)",
                    extensions: &["vanguard"],
                }],
            },
            cx,
        );

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("vanguard");
            }

            let result = cx
                .background_executor()
                .spawn(async move {
                    let payload = archive::export_vanguard_archive(repo.as_ref(), skills.as_ref())?;
                    std::fs::write(&path, payload)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!(
                            "Exported portable workspace archive to {}",
                            path.display()
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Archive export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn import_vanguard_archive(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let skills = self.services.skills.clone();
        let path_rx = Self::prompt_open_single_file(
            OpenFileRequest {
                title: "Import .vanguard Workspace Archive".to_string(),
                initial_dir: Some(Self::initial_save_dir()),
                filters: vec![FileDialogFilter {
                    label: "VanguardCV Archive (*.vanguard)",
                    extensions: &["vanguard"],
                }],
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

            let result = cx
                .background_executor()
                .spawn(async move {
                    let raw = std::fs::read_to_string(&path)?;
                    archive::import_vanguard_archive(&raw, repo.as_ref(), skills.as_ref())
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(outcome) => {
                        let reloaded = load_all_documents(&view.services);
                        if !reloaded.is_empty() {
                            view.documents = reloaded;
                        }
                        if let Some(first_id) = outcome.first_doc_id {
                            view.active_doc_id = Some(first_id);
                        }
                        view.refresh_snapshots();
                        view.trigger_pdf_preview_render(cx);
                        view.notify(format!(
                            "Imported {} master(s), {} variation(s), and {} custom skill(s).",
                            outcome.imported_masters,
                            outcome.imported_variations,
                            outcome.imported_skills
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Archive import failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn export_anonymized_template(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_document().cloned() else {
            self.notify("Select a CV from the library before exporting an anonymized template.");
            return;
        };
        self.flush_pending_now();
        let privacy = self.services.privacy.clone();
        let slug = slugify_skill_id(&doc.title);
        let default_name = format!(
            "{}-template.json",
            if slug.is_empty() { "cv" } else { &slug }
        );
        let initial_dir = Self::initial_save_dir();
        let path_rx = Self::prompt_save_single_file(
            SaveFileRequest {
                title: "Export Anonymized CV Template".to_string(),
                initial_dir: Some(initial_dir),
                default_name: Some(default_name),
                default_extension: Some("json".to_string()),
                filters: vec![FileDialogFilter {
                    label: "JSON Files (*.json)",
                    extensions: &["json"],
                }],
            },
            cx,
        );

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("json");
            }

            let result = cx
                .background_executor()
                .spawn(async move {
                    let json = archive::export_anonymized_template_json(&doc, privacy.as_ref())?;
                    std::fs::write(&path, json)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!(
                            "Exported PII-free anonymized template to {}",
                            path.display()
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Template export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn create_manual_snapshot(&mut self) {
        let Some(active_id) = self.active_doc_id else {
            self.notify("Select a CV from the library before creating a checkpoint.");
            return;
        };
        self.flush_pending_now();
        match self
            .services
            .repository
            .create_snapshot(active_id, "Manual checkpoint")
        {
            Ok(_) => {
                self.refresh_snapshots();
                self.notify("Snapshot checkpoint saved.");
            }
            Err(err) => {
                self.notify_error(format!("Snapshot failed: {err}"));
            }
        }
    }

    pub fn restore_snapshot_by_id(&mut self, snapshot_id: Uuid, cx: &mut Context<Self>) {
        self.flush_pending_now();
        match self.services.repository.restore_snapshot(snapshot_id) {
            Ok(restored) => {
                let restored_id = restored.id;
                if let Some(slot) = self.documents.iter_mut().find(|d| d.id == restored_id) {
                    *slot = restored;
                }
                self.proposals.remove(&restored_id);
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);
                self.notify("Restored CV to selected snapshot.");
            }
            Err(err) => {
                self.notify_error(format!("Restore failed: {err}"));
            }
        }
    }


    // -----------------------------------------------------------------------
    // Software Auto-Updater (Windows GitHub Releases)
    // -----------------------------------------------------------------------

    pub(super) fn schedule_startup_update_check(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(4))
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.auto_update_enabled && matches!(view.update_status, UpdateStatus::Idle) {
                    view.check_for_updates(false, cx);
                }
            });
        })
        .detach();
    }

    pub fn toggle_auto_update(&mut self, cx: &mut Context<Self>) {
        self.auto_update_enabled = !self.auto_update_enabled;
        let value = if self.auto_update_enabled {
            "true"
        } else {
            "false"
        };
        if let Err(err) = self
            .services
            .repository
            .save_setting(settings_keys::AUTO_UPDATE_ENABLED, value)
        {
            tracing::warn!("Failed to persist auto-update setting: {err}");
        }

        if self.auto_update_enabled {
            self.notify("Auto-update enabled. VanguardCV will check for new Windows releases automatically.");
            if matches!(
                self.update_status,
                UpdateStatus::Idle | UpdateStatus::Error(_)
            ) {
                self.check_for_updates(false, cx);
            }
        } else {
            self.notify("Auto-update disabled. You can still check for updates manually anytime.");
        }
    }

    pub fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(
            self.update_status,
            UpdateStatus::Checking | UpdateStatus::Downloading | UpdateStatus::Installing
        ) {
            return;
        }

        self.update_generation = self.update_generation.wrapping_add(1);
        let generation = self.update_generation;
        self.update_status = UpdateStatus::Checking;
        self.update_error = None;
        cx.notify();

        let (tx, mut rx) = unbounded_channel::<anyhow::Result<UpdateCheckOutcome>>();
        crate::runtime::handle().spawn(async move {
            let outcome = updater::check_for_updates(LATEST_RELEASE_API_URL, CURRENT_VERSION).await;
            let _ = tx.send(outcome);
        });

        cx.spawn(async move |this, cx| {
            let Some(result) = rx.recv().await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                if view.update_generation != generation {
                    return;
                }
                match result {
                    Ok(UpdateCheckOutcome::UpToDate { current_version }) => {
                        view.update_status = UpdateStatus::UpToDate;
                        view.update_release = None;
                        view.update_downloaded = None;
                        view.update_error = None;
                        if manual {
                            view.notify(format!("VanguardCV v{current_version} is up to date."));
                        }
                    }
                    Ok(UpdateCheckOutcome::UpdateAvailable(info)) => {
                        let already_ready = view.update_downloaded.as_ref().is_some_and(|d| {
                            d.version == info.version && d.installer_path.is_file()
                        });
                        view.update_release = Some(info.clone());
                        view.update_error = None;
                        if already_ready {
                            view.update_status = UpdateStatus::ReadyToInstall;
                        } else {
                            view.update_downloaded = None;
                            view.update_status = UpdateStatus::Available;
                            if view.auto_update_enabled {
                                view.download_available_update(false, cx);
                            } else if manual {
                                view.notify(format!(
                                    "VanguardCV v{} is available to download.",
                                    info.version
                                ));
                            }
                        }
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        if manual {
                            view.update_status = UpdateStatus::Error(msg.clone());
                            view.update_error = Some(msg.clone());
                            view.notify_error(msg);
                        } else {
                            tracing::warn!("Background update check skipped: {msg}");
                            view.update_status = UpdateStatus::Idle;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn download_available_update(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(
            self.update_status,
            UpdateStatus::Downloading | UpdateStatus::Installing
        ) {
            return;
        }
        let Some(release) = self.update_release.clone() else {
            if manual {
                self.check_for_updates(true, cx);
            }
            return;
        };

        self.update_generation = self.update_generation.wrapping_add(1);
        let generation = self.update_generation;
        self.update_status = UpdateStatus::Downloading;
        self.update_error = None;
        cx.notify();

        let staging_dir = updater::default_update_staging_dir();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<DownloadedUpdate>>();
        crate::runtime::handle().spawn(async move {
            let outcome = updater::download_and_verify_installer(&release, &staging_dir).await;
            let _ = tx.send(outcome);
        });

        cx.spawn(async move |this, cx| {
            let Some(result) = rx.recv().await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                if view.update_generation != generation {
                    return;
                }
                match result {
                    Ok(downloaded) => {
                        let ver = downloaded.version.clone();
                        view.update_downloaded = Some(downloaded);
                        view.update_status = UpdateStatus::ReadyToInstall;
                        view.update_error = None;
                        view.notify(format!(
                            "VanguardCV v{ver} downloaded and verified. Click Install & restart in Settings when ready."
                        ));
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        if manual {
                            view.update_status = UpdateStatus::Error(msg.clone());
                            view.update_error = Some(msg.clone());
                            view.notify_error(msg);
                        } else {
                            tracing::warn!("Background update download failed: {msg}");
                            view.update_status = UpdateStatus::Available;
                            view.update_error = Some(msg);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn apply_downloaded_update(&mut self, cx: &mut Context<Self>) {
        let Some(downloaded) = self.update_downloaded.clone() else {
            self.notify_error("No verified update installer is ready yet.");
            return;
        };
        if matches!(self.update_status, UpdateStatus::Installing) {
            return;
        }

        // Flush any pending SQLite edits safely before handing off to the installer.
        self.flush_pending_now();
        self.update_status = UpdateStatus::Installing;
        self.update_error = None;
        cx.notify();

        match updater::launch_windows_installer(&downloaded.installer_path) {
            Ok(()) => {
                cx.quit();
            }
            Err(err) => {
                let msg = format!("Could not start update installer: {err}");
                self.update_status = UpdateStatus::Error(msg.clone());
                self.update_error = Some(msg.clone());
                self.notify_error(msg);
                cx.notify();
            }
        }
    }

    pub fn open_releases_page(&mut self) {
        let url = self
            .update_release
            .as_ref()
            .map(|r| r.html_url.clone())
            .unwrap_or_else(|| RELEASES_PAGE_URL.to_string());
        self.open_url_or_notify(&url, "releases page");
    }
}