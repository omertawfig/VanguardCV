use gpui::*;
use std::borrow::Cow;

const BRAND_LOGO_PNG: &[u8] = include_bytes!("../../assets/branding/logo.png");

pub struct VanguardAssets;

impl AssetSource for VanguardAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == "branding/logo.png" {
            return Ok(Some(Cow::Borrowed(BRAND_LOGO_PNG)));
        }

        let svg = match path {
            "icons/folder.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1Z" />"#,
                r#"</svg>"#
            )),
            "icons/file.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M6 3h8l4 4v14H6Z" /><path d="M14 3v5h4M9 13h6M9 17h4" />"#,
                r#"</svg>"#
            )),
            "icons/edit.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m14 5 5 5M4 20l5-1L20 8a2 2 0 0 0-5-5L4 14Z" /><path d="M4 14 9 19" />"#,
                r#"</svg>"#
            )),
            "icons/sparkles.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m11 3 2.1 5.9L19 11l-5.9 2.1L11 19l-2.1-5.9L3 11l5.9-2.1Z" /><path d="M19 2v4m-2-2h4M20 17v4m-2-2h4" />"#,
                r#"</svg>"#
            )),
            "icons/shield.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m12 3 8 3v5c0 5-3.4 8.1-8 10-4.6-1.9-8-5-8-10V6Z" /><path d="m8.5 12 2.2 2.2 4.8-4.8" />"#,
                r#"</svg>"#
            )),
            "icons/settings.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m9 3-.5 3-2 1.2-2.8-1.1-2 3.6L4 12l-2.3 2.3 2 3.6 2.8-1.1 2 1.2.5 3h6l.5-3 2-1.2 2.8 1.1 2-3.6L20 12l2.3-2.3-2-3.6-2.8 1.1-2-1.2L15 3Z" />"#,
                r#"<circle cx="12" cy="12" r="3" />"#,
                r#"</svg>"#
            )),
            "icons/chevron.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m9 5 7 7-7 7" />"#,
                r#"</svg>"#
            )),
            "icons/arrow.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M4 12h16m-6-6 6 6-6 6" />"#,
                r#"</svg>"#
            )),
            "icons/branch.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="6" cy="5" r="2" /><circle cx="18" cy="6" r="2" /><circle cx="6" cy="19" r="2" />"#,
                r#"<path d="M6 7v10M8 14h3a7 7 0 0 0 7-6" />"#,
                r#"</svg>"#
            )),
            "icons/plus.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M12 5v14M5 12h14" />"#,
                r#"</svg>"#
            )),
            "icons/search.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="10.5" cy="10.5" r="6.5" /><path d="m16 16 4.5 4.5" />"#,
                r#"</svg>"#
            )),
            "icons/lock.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="5" y="10" width="14" height="11" rx="2" /><path d="M8 10V7a4 4 0 0 1 8 0v3M12 14v3" />"#,
                r#"</svg>"#
            )),
            "icons/check.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m5 12 4 4L19 6" />"#,
                r#"</svg>"#
            )),
            "icons/close.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m6 6 12 12M18 6 6 18" />"#,
                r#"</svg>"#
            )),
            "icons/download.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M12 3v12m-5-5 5 5 5-5M4 17v4h16v-4" />"#,
                r#"</svg>"#
            )),
            "icons/sun.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="12" cy="12" r="4" />"#,
                r#"<path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.4 1.4m11.2 11.2L19 19M5 19l1.4-1.4M17.6 6.4 19 5" />"#,
                r#"</svg>"#
            )),
            "icons/moon.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M20 15.5A8 8 0 0 1 8.5 4 8 8 0 1 0 20 15.5Z" />"#,
                r#"</svg>"#
            )),
            "icons/code.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m8 7-5 5 5 5m8-10 5 5-5 5m-3-13-2 16" />"#,
                r#"</svg>"#
            )),
            "icons/up.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M12 20V4m-6 6 6-6 6 6" />"#,
                r#"</svg>"#
            )),
            "icons/down.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M12 4v16m-6-6 6 6 6-6" />"#,
                r#"</svg>"#
            )),
            "icons/trash.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7" />"#,
                r#"</svg>"#
            )),
            "icons/grip.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="9" cy="5" r="1.5" fill="currentColor"/><circle cx="15" cy="5" r="1.5" fill="currentColor"/>"#,
                r#"<circle cx="9" cy="12" r="1.5" fill="currentColor"/><circle cx="15" cy="12" r="1.5" fill="currentColor"/>"#,
                r#"<circle cx="9" cy="19" r="1.5" fill="currentColor"/><circle cx="15" cy="19" r="1.5" fill="currentColor"/>"#,
                r#"</svg>"#
            )),
            "icons/minus.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M5 12h14" />"#,
                r#"</svg>"#
            )),
            "icons/copy.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V4H4v12h4" />"#,
                r#"</svg>"#
            )),
            "icons/briefcase.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="3" y="7" width="18" height="13" rx="2" /><path d="M8 7V4h8v3M3 12a24 24 0 0 0 18 0M10 12v3h4v-3" />"#,
                r#"</svg>"#
            )),
            "icons/graduation.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="m2 9 10-5 10 5-10 5ZM6 11v6c4 3 8 3 12 0v-6M22 9v8" />"#,
                r#"</svg>"#
            )),
            "icons/list.svg" => Some(concat!(
                r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M9 6h12M9 12h12M9 18h12M3 6h.01M3 12h.01M3 18h.01" />"#,
                r#"</svg>"#
            )),
            _ => None,
        };

        Ok(svg.map(|s| Cow::Borrowed(s.as_bytes())))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![
            "branding/logo.png".into(),
            "icons/folder.svg".into(),
            "icons/file.svg".into(),
            "icons/edit.svg".into(),
            "icons/sparkles.svg".into(),
            "icons/shield.svg".into(),
            "icons/settings.svg".into(),
            "icons/chevron.svg".into(),
            "icons/arrow.svg".into(),
            "icons/branch.svg".into(),
            "icons/plus.svg".into(),
            "icons/search.svg".into(),
            "icons/lock.svg".into(),
            "icons/check.svg".into(),
            "icons/close.svg".into(),
            "icons/download.svg".into(),
            "icons/sun.svg".into(),
            "icons/moon.svg".into(),
            "icons/code.svg".into(),
            "icons/up.svg".into(),
            "icons/down.svg".into(),
            "icons/trash.svg".into(),
            "icons/grip.svg".into(),
            "icons/minus.svg".into(),
            "icons/copy.svg".into(),
            "icons/briefcase.svg".into(),
            "icons/graduation.svg".into(),
            "icons/list.svg".into(),
        ])
    }
}
