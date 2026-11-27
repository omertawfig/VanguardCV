use super::*;

impl WorkspaceView {
    // -----------------------------------------------------------------------
    // Editor CRUD: Rename CV, Sections, Summary, Entries, Bullets, Categories
    // -----------------------------------------------------------------------

    pub fn open_rename_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(title) = self.active_document().map(|d| d.title.clone()) else {
            return;
        };
        self.rename_title_input.update(cx, |i, cx| {
            i.set_value(title);
            cx.notify();
        });
        self.active_dialog = Some(Dialog::RenameCv);
    }

    pub fn save_rename(&mut self, cx: &mut Context<Self>) {
        let new_title = self.rename_title_input.read(cx).text().trim().to_string();
        if !new_title.is_empty() {
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|d| Some(d.id) == self.active_doc_id)
            {
                doc.title = new_title;
            }
            self.persist_active_doc(cx);
            self.notify("CV renamed.");
        }
        self.active_dialog = None;
    }

    pub fn open_add_section_dialog(&mut self, cx: &mut Context<Self>) {
        self.sec_title_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.sec_type_selection = "Timeline entries".to_string();
        self.active_dialog = Some(Dialog::AddSection);
    }

    pub fn execute_add_section(&mut self, cx: &mut Context<Self>) {
        let title = self.sec_title_input.read(cx).text().trim().to_string();
        let final_title = if title.is_empty() {
            "New Section".to_string()
        } else {
            title
        };

        let kind = match self.sec_type_selection.as_str() {
            "Paragraph summary" => SectionKind::Summary {
                text: String::new(),
            },
            "Categorized skills" => SectionKind::CategorizedInlineList {
                categories: Vec::new(),
            },
            "Bullet list" => SectionKind::SimpleBulletList { items: Vec::new() },
            _ => SectionKind::TimelineEntries {
                entries: Vec::new(),
            },
        };

        let new_sec = CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: final_title.clone(),
            enabled: true,
            kind,
        };
        let sec_id = new_sec.id;

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.body.sections.push(new_sec);
        }
        self.expanded_sections.insert(sec_id, true);
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(format!("Added section \"{final_title}\"."));
    }

    pub fn open_rename_section_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        let current = self
            .active_document()
            .and_then(|doc| doc.body.sections.iter().find(|s| s.id == section_id))
            .map(|s| s.title.clone())
            .unwrap_or_default();
        self.rename_section_input.update(cx, |i, cx| {
            i.set_value(current);
            cx.notify();
        });
        self.active_dialog = Some(Dialog::RenameSection { section_id });
    }

    pub fn save_rename_section(&mut self, cx: &mut Context<Self>) {
        let section_id = match self.active_dialog {
            Some(Dialog::RenameSection { section_id }) => section_id,
            _ => return,
        };
        let new_title = self.rename_section_input.read(cx).text().trim().to_string();
        if !new_title.is_empty() {
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|d| Some(d.id) == self.active_doc_id)
                && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            {
                sec.title = new_title;
            }
            self.persist_active_doc(cx);
            self.notify("Section renamed.");
        }
        self.active_dialog = None;
    }

    pub fn delete_section(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.body.sections.retain(|s| s.id != section_id);
        }
        self.expanded_sections.remove(&section_id);
        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Section removed.");
    }

    pub fn open_edit_summary_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        let current = self
            .active_document()
            .and_then(|doc| doc.body.sections.iter().find(|s| s.id == section_id))
            .and_then(|s| match &s.kind {
                SectionKind::Summary { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        self.summary_text_input.update(cx, |i, cx| {
            i.set_value(current);
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSummary { section_id });
    }

    pub fn save_summary_from_dialog(&mut self, cx: &mut Context<Self>) {
        let section_id = match self.active_dialog {
            Some(Dialog::EditSummary { section_id }) => section_id,
            _ => return,
        };
        let new_text = self.summary_text_input.read(cx).text().trim().to_string();
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::Summary { text } = &mut sec.kind
        {
            *text = new_text;
        }
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Summary updated.");
    }

    pub fn open_add_entry_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        self.editing_entry_id = None;
        self.entry_is_current = false;
        self.entry_org_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.entry_role_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.entry_start_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.entry_end_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditEntry {
            section_id,
            is_new: true,
        });
    }

    pub fn open_edit_entry_dialog(
        &mut self,
        section_id: ItemId,
        entry: &TimelineEntry,
        cx: &mut Context<Self>,
    ) {
        self.editing_entry_id = Some(entry.id);
        self.entry_is_current = entry.is_current;
        self.entry_org_input.update(cx, |i, cx| {
            i.set_value(entry.primary_heading.clone());
            cx.notify();
        });
        self.entry_role_input.update(cx, |i, cx| {
            i.set_value(entry.secondary_heading.clone().unwrap_or_default());
            cx.notify();
        });
        self.entry_start_input.update(cx, |i, cx| {
            i.set_value(entry.start_date.clone());
            cx.notify();
        });
        self.entry_end_input.update(cx, |i, cx| {
            i.set_value(entry.end_date.clone().unwrap_or_default());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditEntry {
            section_id,
            is_new: false,
        });
    }

    pub fn save_entry_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (section_id, is_new) = match self.active_dialog {
            Some(Dialog::EditEntry { section_id, is_new }) => (section_id, is_new),
            _ => return,
        };

        let org = self.entry_org_input.read(cx).text().trim().to_string();
        let role = self.entry_role_input.read(cx).text().trim().to_string();
        let start = self.entry_start_input.read(cx).text().trim().to_string();
        let end = self.entry_end_input.read(cx).text().trim().to_string();
        let is_current = self.entry_is_current;

        if org.is_empty() {
            self.notify_error("Organization or institution name is required.");
            return;
        }

        let secondary = if role.is_empty() { None } else { Some(role) };
        let end_date = if is_current || end.is_empty() {
            None
        } else {
            Some(end)
        };

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::TimelineEntries { entries } = &mut sec.kind
        {
            if is_new {
                let mut entry = TimelineEntry::new(org, start);
                entry.secondary_heading = secondary;
                entry.end_date = end_date;
                entry.is_current = is_current;
                self.expanded_entries.insert(entry.id, true);
                entries.push(entry);
            } else if let Some(eid) = self.editing_entry_id
                && let Some(existing) = entries.iter_mut().find(|e| e.id == eid)
            {
                existing.primary_heading = org;
                existing.secondary_heading = secondary;
                existing.start_date = start;
                existing.end_date = end_date;
                existing.is_current = is_current;
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if is_new {
            "Entry added."
        } else {
            "Entry updated."
        });
    }

    pub fn delete_current_entry(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditEntry { section_id, .. }) = self.active_dialog else {
            return;
        };
        let Some(eid) = self.editing_entry_id.take() else {
            return;
        };
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::TimelineEntries { entries } = &mut sec.kind
        {
            entries.retain(|e| e.id != eid);
        }
        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Entry deleted.");
    }

    pub fn open_add_bullet_dialog(
        &mut self,
        section_id: ItemId,
        entry_id: Option<ItemId>,
        cx: &mut Context<Self>,
    ) {
        self.editing_bullet_id = None;
        self.bullet_text_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditBullet {
            section_id,
            entry_id,
            is_new: true,
        });
    }

    pub fn open_edit_bullet_dialog(
        &mut self,
        section_id: ItemId,
        entry_id: Option<ItemId>,
        item: &BulletItem,
        cx: &mut Context<Self>,
    ) {
        self.editing_bullet_id = Some(item.id);
        self.bullet_text_input.update(cx, |i, cx| {
            i.set_value(item.text.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditBullet {
            section_id,
            entry_id,
            is_new: false,
        });
    }

    pub fn save_bullet_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (section_id, entry_id, is_new) = match self.active_dialog {
            Some(Dialog::EditBullet {
                section_id,
                entry_id,
                is_new,
            }) => (section_id, entry_id, is_new),
            _ => return,
        };

        let text = self.bullet_text_input.read(cx).text().trim().to_string();
        if text.is_empty() {
            self.notify_error("Achievement text cannot be empty.");
            return;
        }

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
        {
            match &mut sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    if let Some(eid) = entry_id
                        && let Some(entry) = entries.iter_mut().find(|e| e.id == eid)
                    {
                        if is_new {
                            entry.bullets.push(BulletItem::new(text));
                        } else if let Some(bid) = self.editing_bullet_id
                            && let Some(b) = entry.bullets.iter_mut().find(|b| b.id == bid)
                        {
                            b.text = text;
                        }
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if is_new {
                        items.push(BulletItem::new(text));
                    } else if let Some(bid) = self.editing_bullet_id
                        && let Some(b) = items.iter_mut().find(|b| b.id == bid)
                    {
                        b.text = text;
                    }
                }
                _ => {}
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if is_new {
            "Achievement added."
        } else {
            "Achievement updated."
        });
    }

    pub fn delete_current_bullet(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditBullet {
            section_id,
            entry_id,
            ..
        }) = self.active_dialog
        else {
            return;
        };
        let Some(bid) = self.editing_bullet_id.take() else {
            return;
        };

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
        {
            match &mut sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    if let Some(eid) = entry_id
                        && let Some(entry) = entries.iter_mut().find(|e| e.id == eid)
                    {
                        entry.bullets.retain(|b| b.id != bid);
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    items.retain(|b| b.id != bid);
                }
                _ => {}
            }
        }

        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Achievement deleted.");
    }

    pub fn open_add_category_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        self.editing_category_id = None;
        self.cat_separator = ", ".to_string();
        self.cat_label_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.cat_items_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCategory {
            section_id,
            is_new: true,
        });
    }

    pub fn open_edit_category_dialog(
        &mut self,
        section_id: ItemId,
        cat: &SkillCategoryItem,
        cx: &mut Context<Self>,
    ) {
        self.editing_category_id = Some(cat.id);
        self.cat_separator = if cat.separator.is_empty() {
            ", ".to_string()
        } else {
            cat.separator.clone()
        };
        self.cat_label_input.update(cx, |i, cx| {
            i.set_value(cat.category_label.clone());
            cx.notify();
        });
        self.cat_items_input.update(cx, |i, cx| {
            i.set_value(cat.items.join(", "));
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCategory {
            section_id,
            is_new: false,
        });
    }

    pub fn save_category_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (section_id, is_new) = match self.active_dialog {
            Some(Dialog::EditCategory { section_id, is_new }) => (section_id, is_new),
            _ => return,
        };

        let label = self.cat_label_input.read(cx).text().trim().to_string();
        let items_raw = self.cat_items_input.read(cx).text().to_string();
        if label.is_empty() {
            self.notify_error("Category name is required.");
            return;
        }
        let items: Vec<String> = items_raw
            .split([',', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let separator = self.cat_separator.clone();

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
        {
            if is_new {
                categories.push(SkillCategoryItem {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    enabled: true,
                    category_label: label,
                    separator,
                    items,
                });
            } else if let Some(cid) = self.editing_category_id
                && let Some(existing) = categories.iter_mut().find(|c| c.id == cid)
            {
                existing.category_label = label;
                existing.separator = separator;
                existing.items = items;
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if is_new {
            "Skill category added."
        } else {
            "Skill category updated."
        });
    }

    pub fn delete_current_category(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditCategory { section_id, .. }) = self.active_dialog else {
            return;
        };
        let Some(cid) = self.editing_category_id.take() else {
            return;
        };
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
        {
            categories.retain(|c| c.id != cid);
        }
        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Skill category deleted.");
    }

    pub fn toggle_category_enabled(
        &mut self,
        section_id: ItemId,
        category_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
            && let Some(cat) = categories.iter_mut().find(|c| c.id == category_id)
        {
            cat.enabled = !cat.enabled;
        }
        self.persist_active_doc(cx);
    }

    // -----------------------------------------------------------------------
    // Personal Info (PII Header) CRUD
    // -----------------------------------------------------------------------

    pub fn open_edit_contact_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.active_document().map(|d| d.pii_header.clone()) else {
            return;
        };
        self.pii_name_input.update(cx, |i, cx| {
            i.set_value(h.full_name);
            cx.notify();
        });
        self.pii_headline_input.update(cx, |i, cx| {
            i.set_value(h.headline_job_title.unwrap_or_default());
            cx.notify();
        });
        self.pii_email_input.update(cx, |i, cx| {
            i.set_value(h.email.unwrap_or_default());
            cx.notify();
        });
        self.pii_phone_input.update(cx, |i, cx| {
            i.set_value(h.phone.unwrap_or_default());
            cx.notify();
        });
        self.pii_location_input.update(cx, |i, cx| {
            i.set_value(h.location.unwrap_or_default());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditContact);
    }

    pub fn save_contact_from_dialog(&mut self, cx: &mut Context<Self>) {
        let full_name = self.pii_name_input.read(cx).text().trim().to_string();
        let headline = self.pii_headline_input.read(cx).text().trim().to_string();
        let email = self.pii_email_input.read(cx).text().trim().to_string();
        let phone = self.pii_phone_input.read(cx).text().trim().to_string();
        let location = self.pii_location_input.read(cx).text().trim().to_string();

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.pii_header.full_name = full_name;
            doc.pii_header.headline_job_title = (!headline.is_empty()).then_some(headline);
            doc.pii_header.email = (!email.is_empty()).then_some(email);
            doc.pii_header.phone = (!phone.is_empty()).then_some(phone);
            doc.pii_header.location = (!location.is_empty()).then_some(location);
        }
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Contact header updated locally.");
    }

    pub fn open_add_social_dialog(&mut self, cx: &mut Context<Self>) {
        self.social_platform_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.social_display_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.social_url_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSocialLink { link_id: None });
    }

    pub fn open_edit_social_dialog(&mut self, link: &SocialLink, cx: &mut Context<Self>) {
        self.social_platform_input.update(cx, |i, cx| {
            i.set_value(link.platform.clone());
            cx.notify();
        });
        self.social_display_input.update(cx, |i, cx| {
            i.set_value(link.display_text.clone());
            cx.notify();
        });
        self.social_url_input.update(cx, |i, cx| {
            i.set_value(link.url.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSocialLink {
            link_id: Some(link.id),
        });
    }

    pub fn save_social_link_from_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditSocialLink { link_id }) = self.active_dialog else {
            return;
        };
        let platform = self
            .social_platform_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let display = self.social_display_input.read(cx).text().trim().to_string();
        let url = self.social_url_input.read(cx).text().trim().to_string();

        if platform.is_empty() && url.is_empty() {
            self.notify_error("Provide at least a platform name and URL.");
            return;
        }
        let final_platform = if platform.is_empty() {
            "Link".to_string()
        } else {
            platform
        };
        let final_display = if display.is_empty() {
            url.clone()
        } else {
            display
        };
        let final_url = if url.is_empty() {
            final_display.clone()
        } else {
            url
        };

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            if let Some(id) = link_id {
                if let Some(existing) = doc.pii_header.social_links.iter_mut().find(|l| l.id == id)
                {
                    existing.platform = final_platform;
                    existing.display_text = final_display;
                    existing.url = final_url;
                }
            } else {
                doc.pii_header.social_links.push(SocialLink {
                    id: Uuid::new_v4(),
                    platform: final_platform,
                    display_text: final_display,
                    url: final_url,
                    enabled: true,
                });
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if link_id.is_some() {
            "Social link updated."
        } else {
            "Social link added."
        });
    }

    pub fn toggle_social_link_enabled(&mut self, link_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(link) = doc
                .pii_header
                .social_links
                .iter_mut()
                .find(|l| l.id == link_id)
        {
            link.enabled = !link.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn delete_social_link(&mut self, link_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.pii_header.social_links.retain(|l| l.id != link_id);
        }
        self.persist_active_doc(cx);
        self.notify("Social link removed.");
    }

    pub fn open_add_custom_field_dialog(&mut self, cx: &mut Context<Self>) {
        self.custom_field_label_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.custom_field_value_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCustomField { field_id: None });
    }

    pub fn open_edit_custom_field_dialog(
        &mut self,
        field: &CustomHeaderField,
        cx: &mut Context<Self>,
    ) {
        self.custom_field_label_input.update(cx, |i, cx| {
            i.set_value(field.label.clone());
            cx.notify();
        });
        self.custom_field_value_input.update(cx, |i, cx| {
            i.set_value(field.value.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCustomField {
            field_id: Some(field.id),
        });
    }

    pub fn save_custom_field_from_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditCustomField { field_id }) = self.active_dialog else {
            return;
        };
        let label = self
            .custom_field_label_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let value = self
            .custom_field_value_input
            .read(cx)
            .text()
            .trim()
            .to_string();

        if label.is_empty() || value.is_empty() {
            self.notify_error("Both label and value are required.");
            return;
        }

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            if let Some(id) = field_id {
                if let Some(existing) = doc.pii_header.custom_fields.iter_mut().find(|f| f.id == id)
                {
                    existing.label = label;
                    existing.value = value;
                }
            } else {
                doc.pii_header.custom_fields.push(CustomHeaderField {
                    id: Uuid::new_v4(),
                    label,
                    value,
                    enabled: true,
                });
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if field_id.is_some() {
            "Custom field updated."
        } else {
            "Custom field added."
        });
    }

    pub fn toggle_custom_field_enabled(&mut self, field_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(field) = doc
                .pii_header
                .custom_fields
                .iter_mut()
                .find(|f| f.id == field_id)
        {
            field.enabled = !field.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn delete_custom_field(&mut self, field_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.pii_header.custom_fields.retain(|f| f.id != field_id);
        }
        self.persist_active_doc(cx);
        self.notify("Custom field removed.");
    }


    // -----------------------------------------------------------------------
    // Editor Section / Entry / Bullet Toggles & Reordering
    // -----------------------------------------------------------------------

    pub fn toggle_section_expand(&mut self, sec_id: ItemId) {
        let current = self
            .expanded_sections
            .get(&sec_id)
            .copied()
            .unwrap_or_else(|| {
                self.active_document()
                    .and_then(|doc| doc.body.sections.iter().find(|s| s.id == sec_id))
                    .map(|s| match &s.kind {
                        SectionKind::Summary { .. } => true,
                        SectionKind::TimelineEntries { .. } => {
                            s.title.to_lowercase().contains("experience")
                        }
                        _ => false,
                    })
                    .unwrap_or(false)
            });
        self.expanded_sections.insert(sec_id, !current);
    }

    pub fn toggle_entry_expand(&mut self, entry_id: ItemId) {
        let current = self
            .expanded_entries
            .get(&entry_id)
            .copied()
            .unwrap_or(true);
        self.expanded_entries.insert(entry_id, !current);
    }

    pub fn toggle_section_enabled(&mut self, sec_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == sec_id)
        {
            sec.enabled = !sec.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn begin_editor_drag(&mut self, kind: EditorDragKind) {
        self.active_drag_item = Some(kind);
        self.drag_did_reorder = false;
    }

    pub fn handle_editor_drag_move(
        &mut self,
        dragged: &EditorDragKind,
        target: &EditorDragKind,
        mouse_pos: Point<Pixels>,
        target_bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if dragged == target || !target_bounds.contains(&mouse_pos) {
            return;
        }
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) else {
            return;
        };
        let Some((from_idx, to_idx)) = drag_item_index_pair_in_doc(doc, dragged, target) else {
            return;
        };
        // Directional midpoint hysteresis prevents rapid oscillation across unequal card heights.
        let threshold = target_bounds.size.height * 0.35;
        let crossed = if from_idx < to_idx {
            mouse_pos.y >= target_bounds.top() + threshold
        } else {
            mouse_pos.y <= target_bounds.bottom() - threshold
        };
        if !crossed {
            return;
        }
        if reorder_item_in_doc(doc, dragged, target) {
            self.active_drag_item = Some(dragged.clone());
            self.drag_did_reorder = true;
            cx.notify();
        }
    }

    pub fn finish_editor_drag(&mut self, cx: &mut Context<Self>) {
        let did_reorder = self.drag_did_reorder;
        self.active_drag_item = None;
        self.drag_did_reorder = false;
        if did_reorder {
            self.persist_active_doc(cx);
            cx.notify();
        }
    }

    pub fn toggle_entry_enabled(
        &mut self,
        sec_id: ItemId,
        entry_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == sec_id)
            && let SectionKind::TimelineEntries { entries } = &mut sec.kind
            && let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id)
        {
            entry.enabled = !entry.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn toggle_bullet_enabled(
        &mut self,
        sec_id: ItemId,
        entry_id: Option<ItemId>,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == sec_id)
        {
            match &mut sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    if let Some(eid) = entry_id
                        && let Some(entry) = entries.iter_mut().find(|e| e.id == eid)
                        && let Some(b) = entry.bullets.iter_mut().find(|b| b.id == bullet_id)
                    {
                        b.enabled = !b.enabled;
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if let Some(b) = items.iter_mut().find(|b| b.id == bullet_id) {
                        b.enabled = !b.enabled;
                    }
                }
                _ => {}
            }
        }
        self.persist_active_doc(cx);
    }

    pub fn toggle_variation_skill(&mut self, skill_id: &str) {
        if self.variation_skills.contains(skill_id) {
            self.variation_skills.remove(skill_id);
        } else {
            self.variation_skills.insert(skill_id.to_string());
        }
    }

    pub fn toggle_master_collapse(&mut self, root_id: DocumentId) {
        if self.collapsed_masters.contains(&root_id) {
            self.collapsed_masters.remove(&root_id);
        } else {
            self.collapsed_masters.insert(root_id);
        }
    }

    pub fn toggle_document_menu(&mut self) {
        if self.active_dialog == Some(Dialog::DocumentMenu) {
            self.active_dialog = None;
        } else {
            self.active_dialog = Some(Dialog::DocumentMenu);
        }
    }

    // -----------------------------------------------------------------------
    // Typst Live Preview Rasterization
    // -----------------------------------------------------------------------

    pub fn trigger_pdf_preview_render(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.active_document().cloned() else {
            self.is_rendering_pdf = false;
            self.rendered_images.clear();
            self.preview_error = None;
            return;
        };
        let pdf_engine = self.services.pdf.clone();
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
        let generation = self.pdf_render_generation;

        self.is_rendering_pdf = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { pdf_engine.render_preview_pages(&doc.pii_header, &doc.body, 10) },
                )
                .await;

            let _ = this.update(cx, |view, cx| {
                if view.pdf_render_generation != generation {
                    return;
                }
                view.is_rendering_pdf = false;
                match result {
                    Ok(pages) => {
                        view.rendered_images = pages
                            .into_iter()
                            .filter_map(|p| rendered_page_to_image(&p))
                            .collect();
                        view.preview_error = None;
                    }
                    Err(err) => {
                        view.rendered_images.clear();
                        view.preview_error = Some(err.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

}