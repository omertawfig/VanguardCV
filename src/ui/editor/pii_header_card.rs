//! Editable PII Header card with zero-trust local air-gap styling.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Provides interactive editing of:
//! - Full Name
//! - Headline / Target Role
//! - Email, Phone, Location
//! - Dynamic Social Links (platform, display text, URL, enabled toggle, add/remove)
//! - Dynamic Custom Header Fields (label, value, enabled toggle, add/remove)
//! - Inspect AI Payload affordance (opens local audit modal)

use gpui::{Context, Entity, prelude::*};
use uuid::Uuid;

use crate::contracts::{CustomHeaderField, PiiContactHeader, SocialLink};
use crate::ui::text_input::TextInput;

/// Retained entities for editing the PII header.
pub struct PiiHeaderState {
    pub name_input: Entity<TextInput>,
    pub headline_input: Entity<TextInput>,
    pub email_input: Entity<TextInput>,
    pub phone_input: Entity<TextInput>,
    pub location_input: Entity<TextInput>,
    pub social_links: Vec<SocialLinkField>,
    pub custom_fields: Vec<CustomHeaderFieldEntry>,
}

pub struct SocialLinkField {
    pub id: Uuid,
    pub platform_input: Entity<TextInput>,
    pub display_input: Entity<TextInput>,
    pub url_input: Entity<TextInput>,
    pub enabled: bool,
}

pub struct CustomHeaderFieldEntry {
    pub id: Uuid,
    pub label_input: Entity<TextInput>,
    pub value_input: Entity<TextInput>,
    pub enabled: bool,
}

impl PiiHeaderState {
    /// Creates a new state from a PII header, initializing all input entities.
    pub fn new<V: 'static>(pii: &PiiContactHeader, cx: &mut Context<V>) -> Self {
        let name_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Full Name (e.g. Alex Morgan)")
                .with_content(pii.full_name.clone())
        });

        let headline_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Headline / Professional Title")
                .with_content(pii.headline_job_title.clone().unwrap_or_default())
        });

        let email_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Email address")
                .with_content(pii.email.clone().unwrap_or_default())
        });

        let phone_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Phone number")
                .with_content(pii.phone.clone().unwrap_or_default())
        });

        let location_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Location (e.g. Remote or City, Country)")
                .with_content(pii.location.clone().unwrap_or_default())
        });

        let mut social_links = Vec::new();
        for link in &pii.social_links {
            social_links.push(Self::create_social_link_field(link, cx));
        }

        let mut custom_fields = Vec::new();
        for field in &pii.custom_fields {
            custom_fields.push(Self::create_custom_field_entry(field, cx));
        }

        Self {
            name_input,
            headline_input,
            email_input,
            phone_input,
            location_input,
            social_links,
            custom_fields,
        }
    }

    pub fn create_social_link_field<V: 'static>(
        link: &SocialLink,
        cx: &mut Context<V>,
    ) -> SocialLinkField {
        let platform_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Platform (LinkedIn, GitHub)")
                .with_content(link.platform.clone())
        });

        let display_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Display text (e.g. linkedin.com/in/alex)")
                .with_content(link.display_text.clone())
        });

        let url_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Target URL (https://...)")
                .with_content(link.url.clone())
        });

        SocialLinkField {
            id: link.id,
            platform_input,
            display_input,
            url_input,
            enabled: link.enabled,
        }
    }

    pub fn create_custom_field_entry<V: 'static>(
        field: &CustomHeaderField,
        cx: &mut Context<V>,
    ) -> CustomHeaderFieldEntry {
        let label_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Label (e.g. Clearance)")
                .with_content(field.label.clone())
        });

        let value_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Value (e.g. Top Secret)")
                .with_content(field.value.clone())
        });

        CustomHeaderFieldEntry {
            id: field.id,
            label_input,
            value_input,
            enabled: field.enabled,
        }
    }

    /// Reads back the complete PII header from the UI fields.
    pub fn read_pii(&self, cx: &gpui::App) -> PiiContactHeader {
        let full_name = self.name_input.read(cx).text().trim().to_string();
        let headline_raw = self.headline_input.read(cx).text().trim().to_string();
        let headline_job_title = if headline_raw.is_empty() {
            None
        } else {
            Some(headline_raw)
        };

        let email_raw = self.email_input.read(cx).text().trim().to_string();
        let email = if email_raw.is_empty() {
            None
        } else {
            Some(email_raw)
        };

        let phone_raw = self.phone_input.read(cx).text().trim().to_string();
        let phone = if phone_raw.is_empty() {
            None
        } else {
            Some(phone_raw)
        };

        let location_raw = self.location_input.read(cx).text().trim().to_string();
        let location = if location_raw.is_empty() {
            None
        } else {
            Some(location_raw)
        };

        let mut social_links = Vec::new();
        for link in &self.social_links {
            social_links.push(SocialLink {
                id: link.id,
                platform: link.platform_input.read(cx).text().trim().to_string(),
                display_text: link.display_input.read(cx).text().trim().to_string(),
                url: link.url_input.read(cx).text().trim().to_string(),
                enabled: link.enabled,
            });
        }

        let mut custom_fields = Vec::new();
        for field in &self.custom_fields {
            custom_fields.push(CustomHeaderField {
                id: field.id,
                label: field.label_input.read(cx).text().trim().to_string(),
                value: field.value_input.read(cx).text().trim().to_string(),
                enabled: field.enabled,
            });
        }

        PiiContactHeader {
            full_name,
            headline_job_title,
            email,
            phone,
            location,
            social_links,
            custom_fields,
        }
    }

    /// Every text input in the PII card, used to wire change handlers.
    pub fn inputs(&self) -> Vec<Entity<TextInput>> {
        let mut inputs = vec![
            self.name_input.clone(),
            self.headline_input.clone(),
            self.email_input.clone(),
            self.phone_input.clone(),
            self.location_input.clone(),
        ];
        for link in &self.social_links {
            inputs.push(link.platform_input.clone());
            inputs.push(link.display_input.clone());
            inputs.push(link.url_input.clone());
        }
        for field in &self.custom_fields {
            inputs.push(field.label_input.clone());
            inputs.push(field.value_input.clone());
        }
        inputs
    }
}
