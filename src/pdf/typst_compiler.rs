//! In-memory Typst world, font loading, PDF compilation, and rasterization.
//!
//! The compiler embeds the Libertinus Serif family and the classic template so
//! that PDF output is identical on every machine and never depends on system
//! fonts. Each call builds a fresh [`World`] around the generated source; the
//! expensive standard library and font book are reused.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::{LazyHash, Scalar};
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use crate::contracts::{CvSection, PiiContactHeader, RenderedPage, SanitizedCvBody, SectionKind};

/// Bundled Libertinus Serif faces (SIL Open Font License).
const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/LibertinusSerif-Regular.ttf"),
    include_bytes!("../../assets/fonts/LibertinusSerif-Bold.ttf"),
    include_bytes!("../../assets/fonts/LibertinusSerif-Italic.ttf"),
    include_bytes!("../../assets/fonts/LibertinusSerif-BoldItalic.ttf"),
];

const TEMPLATE: &str = include_str!("../../assets/templates/classic_serif.typ");

/// Pixels per typographic point for raster previews (2.5 = 180 DPI for HiDPI crispness).
const PREVIEW_PIXELS_PER_POINT: f64 = 2.5;

/// Embedded Typst compiler used for both PDF export and preview rasterization.
pub struct TypstCompiler {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

impl TypstCompiler {
    /// Loads the bundled fonts and builds the Typst standard library.
    pub fn new() -> Result<Self> {
        let fonts: Vec<Font> = FONT_FILES
            .iter()
            .flat_map(|data| Font::iter(Bytes::new(*data)))
            .collect();

        if fonts.is_empty() {
            return Err(anyhow!("bundled Libertinus Serif fonts failed to load"));
        }

        let book = FontBook::from_fonts(fonts.iter());

        Ok(Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
        })
    }

    /// Compiles the document and returns the paged layout, logging warnings.
    fn compile_document(
        &self,
        pii: &PiiContactHeader,
        body: &SanitizedCvBody,
    ) -> Result<PagedDocument> {
        let world = CvWorld::new(
            self.library.clone(),
            self.book.clone(),
            self.fonts.clone(),
            build_source(pii, body),
        );

        let warned = typst::compile::<PagedDocument>(&world);
        for warning in &warned.warnings {
            tracing::debug!(message = %warning.message, "typst warning");
        }

        warned.output.map_err(|errors| {
            anyhow!(
                "Typst compilation failed: {}",
                format_diagnostics(errors.as_slice())
            )
        })
    }
}

impl Default for TypstCompiler {
    fn default() -> Self {
        Self::new().expect("bundled Libertinus fonts must load")
    }
}

impl crate::contracts::PdfCompiler for TypstCompiler {
    fn compile_to_pdf(&self, pii: &PiiContactHeader, body: &SanitizedCvBody) -> Result<Vec<u8>> {
        let document = self.compile_document(pii, body)?;
        typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|errors| {
            anyhow!(
                "Typst PDF export failed: {}",
                format_diagnostics(errors.as_slice())
            )
        })
    }

    fn render_preview_pages(
        &self,
        pii: &PiiContactHeader,
        body: &SanitizedCvBody,
        max_pages: usize,
    ) -> Result<Vec<RenderedPage>> {
        if max_pages == 0 {
            return Ok(Vec::new());
        }

        let document = self.compile_document(pii, body)?;
        let options = typst_render::RenderOptions {
            pixel_per_pt: Scalar::new(PREVIEW_PIXELS_PER_POINT),
            render_bleed: false,
        };

        let mut pages = Vec::new();
        for (page_index, page) in document.pages().iter().take(max_pages).enumerate() {
            let pixmap = typst_render::render(page, &options);
            let width_px = pixmap.width();
            let height_px = pixmap.height();

            // tiny-skia stores premultiplied RGBA; the contract wants straight
            // RGBA8 for GPUI.
            let mut rgba = pixmap.data().to_vec();
            unpremultiply_rgba(&mut rgba);

            pages.push(RenderedPage {
                page_index,
                width_px,
                height_px,
                rgba,
            });
        }

        Ok(pages)
    }
}

/// Converts tiny-skia's premultiplied RGBA to straight RGBA.
fn unpremultiply_rgba(data: &mut [u8]) {
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                let value = (u32::from(*channel) * 255 + alpha / 2) / alpha;
                *channel = value.min(255) as u8;
            }
        }
    }
}

/// The minimal in-memory [`World`] required for a single CV compilation.
struct CvWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: FileId,
    source: Source,
}

impl CvWorld {
    fn new(
        library: LazyHash<Library>,
        book: LazyHash<FontBook>,
        fonts: Vec<Font>,
        text: String,
    ) -> Self {
        let main = main_file_id();
        let source = Source::new(main, text);
        Self {
            library,
            book,
            fonts,
            main,
            source,
        }
    }
}

impl World for CvWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(PathBuf::from("<embedded>")))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.source(id)
            .map(|source| Bytes::from_string(source.text().to_string()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

fn main_file_id() -> FileId {
    let path = RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new("main.typ").expect("main.typ is a valid virtual path"),
    );
    FileId::new(path)
}

fn format_diagnostics(errors: &[SourceDiagnostic]) -> String {
    let mut messages: Vec<String> = errors
        .iter()
        .map(|error| error.message.to_string())
        .collect();
    if messages.is_empty() {
        messages.push("unknown Typst error".to_string());
    }
    messages.join("; ")
}

// ---------------------------------------------------------------------------
// Typst source generation
// ---------------------------------------------------------------------------

/// Ensures URLs passed to Typst `#link(...)` always have a valid URI scheme
/// and percent-encoded characters so malformed input never breaks compilation.
fn normalize_link_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") || trimmed.starts_with("mailto:") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&candidate)
        .or_else(|_| url::Url::parse(&candidate.replace(' ', "%20")))
        .ok()?;
    match parsed.scheme() {
        "http" | "https" | "mailto" => Some(parsed.to_string()),
        _ => None,
    }
}

/// Renders the template and a data dictionary into one self-contained source.
fn build_source(pii: &PiiContactHeader, body: &SanitizedCvBody) -> String {
    let mut source = String::with_capacity(8 * 1024);
    source.push_str(TEMPLATE);
    source.push_str("\n\n#cv((\n");

    source.push_str("  name: ");
    source.push_str(&typst_string(&pii.full_name));

    source.push_str(",\n  headline: ");
    source.push_str(&typst_option(pii.headline_job_title.as_deref()));

    let contacts = pii
        .contact_items()
        .into_iter()
        .map(|item| typst_string(&item))
        .collect();
    source.push_str(",\n  contacts: ");
    source.push_str(&typst_array(contacts));

    let links = pii
        .social_links
        .iter()
        .filter(|link| link.enabled && (!link.url.trim().is_empty() || !link.display_text.trim().is_empty()))
        .map(|link| {
            let display = if link.display_text.trim().is_empty() {
                link.url.trim()
            } else {
                link.display_text.trim()
            };
            let url_literal = match normalize_link_url(&link.url) {
                Some(valid) => typst_string(&valid),
                None => "none".to_string(),
            };
            format!(
                "(text: {}, url: {url_literal})",
                typst_string(display),
            )
        })
        .collect();
    source.push_str(",\n  links: ");
    source.push_str(&typst_array(links));

    let sections = body
        .sections
        .iter()
        .filter(|section| section.enabled)
        .filter_map(section_dict)
        .collect();
    source.push_str(",\n  sections: ");
    source.push_str(&typst_array(sections));

    source.push_str(",\n))\n");
    source
}

fn section_dict(section: &CvSection) -> Option<String> {
    let title = typst_string(&section.title);
    match &section.kind {
        SectionKind::Summary { text } => {
            if text.trim().is_empty() {
                return None;
            }
            Some(format!(
                "(title: {title}, kind: \"summary\", text: {})",
                typst_string(text)
            ))
        }
        SectionKind::TimelineEntries { entries } => {
            let entries: Vec<String> = entries
                .iter()
                .filter(|entry| entry.enabled)
                .map(|entry| {
                    let bullets = entry
                        .bullets
                        .iter()
                        .filter(|bullet| bullet.enabled && !bullet.text.trim().is_empty())
                        .map(|bullet| typst_string(&bullet.text))
                        .collect();
                    let tags = entry
                        .tags
                        .iter()
                        .filter(|tag| !tag.trim().is_empty())
                        .map(|tag| typst_string(tag))
                        .collect();
                    format!(
                        "(heading: {}, secondary: {}, dates: {}, bullets: {}, tags: {})",
                        typst_string(&entry.primary_heading),
                        typst_option(entry.secondary_heading.as_deref()),
                        typst_string(&entry.date_range()),
                        typst_array(bullets),
                        typst_array(tags),
                    )
                })
                .collect();
            if entries.is_empty() {
                return None;
            }
            Some(format!(
                "(title: {title}, kind: \"timeline\", entries: {})",
                typst_array(entries)
            ))
        }
        SectionKind::CategorizedInlineList { categories } => {
            let categories: Vec<String> = categories
                .iter()
                .filter(|category| {
                    category.enabled
                        && (!category.category_label.trim().is_empty()
                            || category.items.iter().any(|i| !i.trim().is_empty()))
                })
                .map(|category| {
                    let separator = if category.separator.trim().is_empty() {
                        ", "
                    } else {
                        category.separator.as_str()
                    };
                    let items = category
                        .items
                        .iter()
                        .filter(|item| !item.trim().is_empty())
                        .map(|item| typst_string(item))
                        .collect();
                    format!(
                        "(label: {}, separator: {}, items: {})",
                        typst_string(&category.category_label),
                        typst_string(separator),
                        typst_array(items),
                    )
                })
                .collect();
            if categories.is_empty() {
                return None;
            }
            Some(format!(
                "(title: {title}, kind: \"inline\", categories: {})",
                typst_array(categories)
            ))
        }
        SectionKind::SimpleBulletList { items } => {
            let items: Vec<String> = items
                .iter()
                .filter(|item| item.enabled && !item.text.trim().is_empty())
                .map(|item| typst_string(&item.text))
                .collect();
            if items.is_empty() {
                return None;
            }
            Some(format!(
                "(title: {title}, kind: \"bullets\", items: {})",
                typst_array(items)
            ))
        }
    }
}

/// Escapes a Rust string as a Typst string literal.
fn typst_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{{{:x}}}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn typst_option(value: Option<&str>) -> String {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => typst_string(value),
        None => "none".to_string(),
    }
}

/// Formats a tuple/array literal, including the required trailing comma for a
/// single element.
fn typst_array(items: Vec<String>) -> String {
    match items.len() {
        0 => "()".to_string(),
        1 => format!("({},)", items[0]),
        _ => format!("({})", items.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{BulletItem, PdfCompiler, TimelineEntry};
    use crate::mocks::sample_root_document;

    fn sample() -> (PiiContactHeader, SanitizedCvBody) {
        let doc = sample_root_document();
        (doc.pii_header, doc.body)
    }

    #[test]
    fn typst_string_escapes_markup() {
        assert_eq!(typst_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(typst_string("line\nbreak"), "\"line\\nbreak\"");
        assert_eq!(typst_string("$(x)$ #import"), "\"$(x)$ #import\"");
    }

    #[test]
    fn single_element_array_has_trailing_comma() {
        assert_eq!(typst_array(vec!["\"a\"".into()]), "(\"a\",)");
        assert_eq!(typst_array(Vec::new()), "()");
    }

    #[test]
    fn compiles_sample_to_pdf() {
        let compiler = TypstCompiler::new().unwrap();
        let (pii, body) = sample();
        let pdf = compiler.compile_to_pdf(&pii, &body).unwrap();
        assert!(pdf.starts_with(b"%PDF"), "missing PDF header");
        assert!(pdf.len() > 2_000, "suspiciously small PDF: {}", pdf.len());
    }

    #[test]
    fn preview_pages_are_a4_and_non_blank() {
        let compiler = TypstCompiler::new().unwrap();
        let (pii, body) = sample();
        let pages = compiler.render_preview_pages(&pii, &body, 3).unwrap();
        assert!(!pages.is_empty());

        let page = &pages[0];
        assert!(
            (1475..=1500).contains(&page.width_px),
            "unexpected width {}",
            page.width_px
        );
        assert!(
            (2090..=2120).contains(&page.height_px),
            "unexpected height {}",
            page.height_px
        );
        assert_eq!(
            page.rgba.len(),
            (page.width_px * page.height_px * 4) as usize
        );

        let ink = page
            .rgba
            .as_chunks::<4>().0.iter()
            .filter(|px| px[0] < 200 || px[1] < 200 || px[2] < 200)
            .count();
        assert!(ink > 1_000, "page looks blank (ink pixels: {ink})");
    }

    #[test]
    fn long_document_paginates() {
        let compiler = TypstCompiler::new().unwrap();
        let (pii, mut body) = sample();

        let Some(SectionKind::TimelineEntries { entries }) = body
            .sections
            .iter_mut()
            .find(|section| matches!(section.kind, SectionKind::TimelineEntries { .. }))
            .map(|section| &mut section.kind)
        else {
            panic!("sample has no timeline section");
        };

        let template = entries.first().cloned().expect("sample has an entry");
        for index in 0..24 {
            let mut entry = TimelineEntry::new(
                format!("Long Career Entry {index}"),
                format!("Jan {}", 1990 + index),
            );
            entry.end_date = Some(format!("Dec {}", 1991 + index));
            entry.bullets = (0..4)
                .map(|bullet| {
                    BulletItem::new(format!(
                        "Delivered a substantial line of work number {bullet} that takes up \
                         enough horizontal space to wrap across multiple lines in the PDF."
                    ))
                })
                .collect();
            entries.push(entry);
        }
        entries.push(template);

        let pages = compiler.render_preview_pages(&pii, &body, 10).unwrap();
        assert!(
            pages.len() >= 2,
            "expected multiple pages, got {}",
            pages.len()
        );
    }

    #[test]
    fn injected_pii_compiles() {
        let compiler = TypstCompiler::new().unwrap();
        let (mut pii, body) = sample();
        pii.full_name = "Zoë \"Zed\" O'Conner #1".to_string();
        pii.email = Some("zoe@example.test".to_string());
        pii.phone = Some("+1 (555) 010-9999".to_string());
        pii.social_links[0].display_text = "example.test/zoe".to_string();
        pii.social_links[0].url = "example.test/zoe".to_string(); // schemeless URL

        let pdf = compiler.compile_to_pdf(&pii, &body).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn malformed_social_url_and_empty_contacts_compile() {
        let compiler = TypstCompiler::new().unwrap();
        let (mut pii, body) = sample();
        pii.email = None;
        pii.phone = None;
        pii.location = None;
        pii.custom_fields.clear();
        pii.social_links[0].display_text = "My Portfolio".to_string();
        pii.social_links[0].url = "not a valid :::// url".to_string();

        let pdf = compiler.compile_to_pdf(&pii, &body).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }
}
