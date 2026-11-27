//! Master CV importer for `.docx` (primary) and `.pdf` documents.
//!
//! Workflow:
//! 1. Extract formatted lines from `.docx` (`word/document.xml` + `styles.xml`)
//!    or `.pdf` (`pdf-extract` with Typst/bullet stitching & line-wrap rejoining).
//! 2. Run the default classifier (`classifier::classify_lines`), automatically
//!    recognizing VanguardCV's own Classic Serif output and common CV formats
//!    while strictly skipping the top Personal Information header.
//! 3. Present the resulting [`ImportDraft`] in the interactive Verification UI
//!    so the user can designate any line as a Section start (with layout type),
//!    Entry, Role/Subheading, Point, or Skip, merge split lines, and inspect the
//!    live parsed structure before completing the import.
//! 4. Build a brand-new Master [`CvDocument`] with `parent_id: None` and a
//!    guaranteed-empty [`PiiContactHeader::default()`] (personal info is never
//!    parsed and must be entered manually in the app).

pub mod classifier;
pub mod docx;
pub mod pdf;

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::contracts::{CvDocument, PiiContactHeader, SanitizedCvBody, SectionKind};

/// Supported source file formats for Master CV import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSourceFormat {
    Docx,
    Pdf,
}

impl ImportSourceFormat {
    pub fn label(self) -> &'static str {
        match self {
            ImportSourceFormat::Docx => "DOCX",
            ImportSourceFormat::Pdf => "PDF",
        }
    }
}

/// Lightweight layout selector for a section start line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSectionKind {
    Summary,
    Timeline,
    Skills,
    Bullets,
}

impl ImportSectionKind {
    pub fn label(self) -> &'static str {
        match self {
            ImportSectionKind::Summary => "Summary",
            ImportSectionKind::Timeline => "Timeline",
            ImportSectionKind::Skills => "Skills",
            ImportSectionKind::Bullets => "Bullets",
        }
    }

    pub const ALL: [ImportSectionKind; 4] = [
        ImportSectionKind::Timeline,
        ImportSectionKind::Bullets,
        ImportSectionKind::Summary,
        ImportSectionKind::Skills,
    ];
}

/// Interactive role assigned to an extracted document line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportLineRole {
    /// Excluded from import (e.g., personal info header, page numbers, noise).
    Skip,
    /// Marks the start/title of a new CV section.
    Section { kind: ImportSectionKind },
    /// Starts a chronological entry inside a `Timeline` section (org + dates).
    Entry,
    /// Job title / degree line belonging to the preceding `Entry`.
    EntrySubheading,
    /// Content line: bullet point, summary sentence, or `Label: skills` category.
    Point,
}

impl ImportLineRole {
    pub fn short_label(self) -> &'static str {
        match self {
            ImportLineRole::Skip => "Skip",
            ImportLineRole::Section { .. } => "Section",
            ImportLineRole::Entry => "Entry",
            ImportLineRole::EntrySubheading => "Role",
            ImportLineRole::Point => "Point",
        }
    }
}

/// Raw line extracted from DOCX or PDF along with formatting hints.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedLine {
    pub text: String,
    pub is_bold: bool,
    pub is_heading: bool,
    pub is_list_item: bool,
    pub has_bottom_border: bool,
    pub font_size_pt: Option<f32>,
    pub is_all_caps: bool,
    pub is_table_row: bool,
}

impl ExtractedLine {
    pub fn from_text(
        text: String,
        is_bold: bool,
        is_heading: bool,
        is_list_item: bool,
        has_bottom_border: bool,
        font_size_pt: Option<f32>,
        is_table_row: bool,
    ) -> Self {
        let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        let is_all_caps =
            letters.len() >= 3 && letters.iter().all(|c| c.is_uppercase());
        Self {
            text,
            is_bold,
            is_heading,
            is_list_item,
            has_bottom_border,
            font_size_pt,
            is_all_caps,
            is_table_row,
        }
    }
}

/// Single reviewable line in the interactive Import Verification dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportLine {
    pub id: usize,
    pub raw: ExtractedLine,
    pub text: String,
    pub role: ImportLineRole,
    pub auto_role: ImportLineRole,
    pub is_likely_pii_header: bool,
    pub user_modified: bool,
}

/// Live summary statistics shown in the Import Verification dialog.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub sections: usize,
    pub entries: usize,
    pub points: usize,
    pub skipped: usize,
    pub pii_header_skipped: usize,
}

/// Editable draft produced by parsing a `.docx` or `.pdf` file and verified by
/// the user prior to persisting a Master CV into SQLite.
#[derive(Debug, Clone)]
pub struct ImportDraft {
    pub source_file_name: String,
    pub suggested_title: String,
    pub source_format: ImportSourceFormat,
    pub detected_vanguard_format: bool,
    pub warnings: Vec<String>,
    pub lines: Vec<ImportLine>,
    cached_body: SanitizedCvBody,
    cached_stats: ImportStats,
}

impl ImportDraft {
    /// Parses a `.docx` or `.pdf` file on disk into an interactive [`ImportDraft`].
    pub fn from_file(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let format = match ext.as_str() {
            "docx" => ImportSourceFormat::Docx,
            "pdf" => ImportSourceFormat::Pdf,
            "" => {
                bail!("Unsupported file: please choose a .docx or .pdf CV file.");
            }
            other => {
                bail!(
                    "Unsupported file format '.{other}'. Master CV import supports .docx and .pdf files."
                );
            }
        };

        const MAX_IMPORT_FILE_BYTES: u64 = 25 * 1024 * 1024; // 25 MiB
        let meta = std::fs::metadata(path)
            .with_context(|| format!("Could not inspect {}", path.display()))?;
        if meta.len() > MAX_IMPORT_FILE_BYTES {
            bail!(
                "Selected file is too large to import ({:.1} MB; maximum supported size is 25 MB).",
                meta.len() as f64 / (1024.0 * 1024.0)
            );
        }

        let bytes = std::fs::read(path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Imported CV")
            .to_string();

        match format {
            ImportSourceFormat::Docx => Self::from_docx_bytes(&file_name, &bytes),
            ImportSourceFormat::Pdf => Self::from_pdf_bytes(&file_name, &bytes),
        }
    }

    fn from_classified_lines(
        file_name: &str,
        source_format: ImportSourceFormat,
        detected_vanguard_format: bool,
        warnings: Vec<String>,
        lines: Vec<ImportLine>,
    ) -> Self {
        let (cached_body, cached_stats) = compute_body_and_stats(&lines);
        Self {
            source_file_name: file_name.to_string(),
            suggested_title: suggest_title_from_filename(file_name),
            source_format,
            detected_vanguard_format,
            warnings,
            lines,
            cached_body,
            cached_stats,
        }
    }

    fn refresh_cache(&mut self) {
        let (body, stats) = compute_body_and_stats(&self.lines);
        self.cached_body = body;
        self.cached_stats = stats;
    }

    /// Parses raw `.docx` bytes into an interactive [`ImportDraft`].
    pub fn from_docx_bytes(file_name: &str, bytes: &[u8]) -> Result<Self> {
        let raw_lines = docx::extract_docx_lines(bytes)?;
        let (lines, detected_vanguard_format) = classifier::classify_lines(raw_lines);
        Ok(Self::from_classified_lines(
            file_name,
            ImportSourceFormat::Docx,
            detected_vanguard_format,
            Vec::new(),
            lines,
        ))
    }

    /// Parses raw `.pdf` bytes into an interactive [`ImportDraft`].
    pub fn from_pdf_bytes(file_name: &str, bytes: &[u8]) -> Result<Self> {
        let (raw_lines, warnings) = pdf::extract_pdf_lines(bytes)?;
        let (lines, detected_vanguard_format) = classifier::classify_lines(raw_lines);
        Ok(Self::from_classified_lines(
            file_name,
            ImportSourceFormat::Pdf,
            detected_vanguard_format,
            warnings,
            lines,
        ))
    }

    /// Assigns a new role to line `index`.
    ///
    /// When the user promotes a line to `Section`, any unedited lines immediately
    /// below it (up to the next `Section`) that were auto-skipped solely because
    /// no section existed above them yet automatically flow into that new section
    /// as points/entries — matching the user's expectation that marking a section
    /// start pulls the lines underneath into that section.
    pub fn set_line_role(&mut self, index: usize, role: ImportLineRole) {
        if index >= self.lines.len() {
            return;
        }
        let target_role = match role {
            ImportLineRole::Section { .. } => {
                // Keep existing kind if already a Section, otherwise infer from the line title.
                if let ImportLineRole::Section { kind } = self.lines[index].role {
                    ImportLineRole::Section { kind }
                } else {
                    let inferred = classifier::infer_section_kind_for_title(&self.lines[index].text);
                    ImportLineRole::Section { kind: inferred }
                }
            }
            other => other,
        };

        self.lines[index].role = target_role;
        self.lines[index].user_modified = true;

        if let ImportLineRole::Section { kind } = target_role {
            self.lines[index].is_likely_pii_header = false;
            // Flow unedited auto-skipped lines below this newly designated section
            // (up to the next Section) into the new section so the user doesn't
            // have to click every single bullet underneath one by one.
            let mut prev_was_entry = false;
            for line in self.lines.iter_mut().skip(index + 1) {
                if matches!(line.role, ImportLineRole::Section { .. }) {
                    break;
                }
                if !line.user_modified && line.role == ImportLineRole::Skip {
                    line.is_likely_pii_header = false;
                    line.role = match kind {
                        ImportSectionKind::Timeline => {
                            if line.raw.is_list_item {
                                prev_was_entry = false;
                                ImportLineRole::Point
                            } else if classifier::line_contains_date_range(&line.text) {
                                prev_was_entry = true;
                                ImportLineRole::Entry
                            } else if prev_was_entry
                                && line.text.len() <= 90
                                && !line.text.ends_with('.')
                            {
                                prev_was_entry = false;
                                ImportLineRole::EntrySubheading
                            } else {
                                prev_was_entry = false;
                                ImportLineRole::Point
                            }
                        }
                        _ => ImportLineRole::Point,
                    };
                }
            }
        }
        self.refresh_cache();
    }

    /// Updates the layout kind (`Timeline`, `Bullets`, `Summary`, `Skills`) on a `Section` line.
    pub fn set_section_kind(&mut self, index: usize, kind: ImportSectionKind) {
        if let Some(line) = self.lines.get_mut(index) {
            line.role = ImportLineRole::Section { kind };
            line.is_likely_pii_header = false;
            line.user_modified = true;
            self.refresh_cache();
        }
    }

    /// Merges line `index` onto the end of line `index - 1` (handy for fixing
    /// accidental mid-sentence line wraps in PDFs).
    pub fn merge_line_up(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.lines.len() {
            return false;
        }
        let child_text = self.lines[index].text.trim().to_string();
        if child_text.is_empty() {
            self.lines.remove(index);
            self.refresh_cache();
            return true;
        }
        let prev = &mut self.lines[index - 1];
        if prev.text.ends_with('-') && !prev.text.ends_with(" -") {
            prev.text.pop();
            prev.text.push_str(&child_text);
        } else {
            if !prev.text.is_empty() {
                prev.text.push(' ');
            }
            prev.text.push_str(&child_text);
        }
        prev.user_modified = true;
        self.lines.remove(index);
        self.refresh_cache();
        true
    }

    /// Marks every line above the first `Section` as `Skip` (Personal Info Header).
    pub fn skip_all_above_first_section(&mut self) {
        let first_sec = self
            .lines
            .iter()
            .position(|l| matches!(l.role, ImportLineRole::Section { .. }));
        let Some(limit) = first_sec else {
            return;
        };
        for line in self.lines.iter_mut().take(limit) {
            line.role = ImportLineRole::Skip;
            line.is_likely_pii_header = true;
            line.user_modified = true;
        }
        self.refresh_cache();
    }

    /// Resets every line's role back to the default automatic parser decision.
    pub fn reset_to_auto(&mut self) {
        for line in &mut self.lines {
            line.role = line.auto_role;
            line.user_modified = false;
        }
        self.refresh_cache();
    }

    /// Returns the cached compiled `SanitizedCvBody` for zero-cost per-frame rendering.
    pub fn cached_body(&self) -> &SanitizedCvBody {
        &self.cached_body
    }

    /// Returns the cached [`ImportStats`] for zero-cost per-frame rendering.
    pub fn cached_stats(&self) -> ImportStats {
        self.cached_stats
    }

    /// Compiles the current verified line roles into a `SanitizedCvBody`.
    pub fn build_body(&self) -> SanitizedCvBody {
        self.cached_body.clone()
    }

    /// Returns live stats for the verification preview header.
    pub fn stats(&self) -> ImportStats {
        self.cached_stats
    }

    /// Produces the final Master [`CvDocument`] ready to persist.
    ///
    /// Always sets `parent_id = None` and `pii_header = PiiContactHeader::default()`
    /// so personal information is never imported from files.
    pub fn build_master_document(&self, title: impl Into<String>) -> CvDocument {
        let raw_title = title.into();
        let clean_title = if raw_title.trim().is_empty() {
            self.suggested_title.clone()
        } else {
            raw_title.trim().to_string()
        };
        let mut doc = CvDocument::new_root(clean_title);
        doc.pii_header = PiiContactHeader::default();
        doc.body = self.build_body();
        doc
    }
}

fn compute_body_and_stats(lines: &[ImportLine]) -> (SanitizedCvBody, ImportStats) {
    let body = classifier::build_body_from_lines(lines);
    let mut entries = 0usize;
    let mut points = 0usize;
    for sec in &body.sections {
        match &sec.kind {
            SectionKind::Summary { text } => {
                if !text.trim().is_empty() {
                    points += 1;
                }
            }
            SectionKind::TimelineEntries { entries: list } => {
                entries += list.len();
                for e in list {
                    points += e.bullets.len();
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                points += categories.len();
            }
            SectionKind::SimpleBulletList { items } => {
                points += items.len();
            }
        }
    }
    let skipped = lines
        .iter()
        .filter(|l| l.role == ImportLineRole::Skip)
        .count();
    let pii_header_skipped = lines
        .iter()
        .filter(|l| l.role == ImportLineRole::Skip && l.is_likely_pii_header)
        .count();

    let stats = ImportStats {
        sections: body.sections.len(),
        entries,
        points,
        skipped,
        pii_header_skipped,
    };
    (body, stats)
}

fn suggest_title_from_filename(file_name: &str) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported Master CV");
    let cleaned = stem
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        "Imported Master CV".to_string()
    } else {
        cleaned
    }
}

/// Normalizes ligatures, non-breaking spaces, zero-width characters, and runs of spaces
/// while preserving intentional multi-space column gaps (`"   "`).
pub(crate) fn normalize_line_text(input: &str) -> String {
    let replaced = input
        .replace('\u{00A0}', " ")
        .replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
        .replace('\u{FB00}', "ff")
        .replace('\u{FB01}', "fi")
        .replace('\u{FB02}', "fl")
        .replace('\u{FB03}', "ffi")
        .replace('\u{FB04}', "ffl")
        .replace('\u{F0B7}', "•");

    // Collapse tabs into 3 spaces (column separator) and normalize excessive spaces
    // while keeping 3-space column gaps intact for date/heading separation.
    let mut out = String::with_capacity(replaced.len());
    let mut space_run = 0usize;
    for ch in replaced.chars() {
        if ch == '\t' {
            if space_run < 3 {
                for _ in 0..(3 - space_run) {
                    out.push(' ');
                }
            }
            space_run = 3;
        } else if ch.is_whitespace() {
            space_run += 1;
            if space_run <= 3 {
                out.push(' ');
            }
        } else {
            space_run = 0;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

/// Strips a leading bullet marker (`•`, `-`, `*`, `▪`, `◦`, `‣`, `●`, `▸`) if present.
pub(crate) fn strip_leading_bullet_marker(text: &str) -> (String, bool) {
    let trimmed = text.trim_start();
    let bullet_chars = ['•', '·', '▪', '▫', '◦', '‣', '●', '○', '▸', '▹', '–', '—', '-', '*'];
    if let Some(first) = trimmed.chars().next()
        && bullet_chars.contains(&first)
    {
        let rest = &trimmed[first.len_utf8()..];
        // Require whitespace after `-`/`*`/`–`/`—` so negative numbers or ranges aren't stripped.
        let needs_space = matches!(first, '-' | '*' | '–' | '—' | '·');
        if !needs_space || rest.starts_with(char::is_whitespace) {
            return (rest.trim().to_string(), true);
        }
    }
    (trimmed.to_string(), false)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::contracts::{CvRepository, PdfCompiler};
    use crate::db::repository::SqliteRepository;
    use crate::db::seed;
    use crate::pdf::typst_compiler::TypstCompiler;

    fn build_test_docx(document_xml: &str) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("word/document.xml", options).unwrap();
            writer.write_all(document_xml.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn parses_structured_docx_skips_pii_header_and_populates_all_section_kinds() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:rPr><w:b/><w:sz w:val="36"/></w:rPr><w:t>Jordan Blake</w:t></w:r></w:p>
    <w:p><w:r><w:t>jordan.blake@example.com | +1 (555) 123-4567 | Remote</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>SUMMARY</w:t></w:r></w:p>
    <w:p><w:r><w:t>Security engineer focused on offensive testing and cloud defense.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>PROFESSIONAL EXPERIENCE</w:t></w:r></w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>CrowdStrike</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Apr 2020 - (Currently)</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Senior Security Analyst</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Automated alert triage in Python, cutting MTTR by 30%.</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Authored 15+ Splunk detection rules for lateral movement.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>TECHNICAL SKILLS</w:t></w:r></w:p>
    <w:p><w:r><w:t>• Languages: Rust, Python, Go</w:t></w:r></w:p>
    <w:p><w:r><w:t>• Security: Burp Suite, Splunk, AWS IAM</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>CERTIFICATIONS</w:t></w:r></w:p>
    <w:p><w:r><w:t>• OSCP — Offensive Security Certified Professional</w:t></w:r></w:p>
  </w:body>
</w:document>"#;

        let bytes = build_test_docx(xml);
        let draft = ImportDraft::from_docx_bytes("Jordan_Security_CV.docx", &bytes)
            .expect("docx parse should succeed");

        assert_eq!(draft.suggested_title, "Jordan Security CV");
        assert!(draft.detected_vanguard_format);

        // Top 2 lines (Name + Contact bar) must be skipped as PII header.
        assert_eq!(draft.lines[0].role, ImportLineRole::Skip);
        assert!(draft.lines[0].is_likely_pii_header);
        assert_eq!(draft.lines[1].role, ImportLineRole::Skip);
        assert!(draft.lines[1].is_likely_pii_header);

        let doc = draft.build_master_document("Imported Master");
        assert!(doc.is_root());
        assert!(doc.pii_header.is_empty(), "PII header must NEVER be parsed");
        assert_eq!(doc.body.sections.len(), 4);

        // 1. Summary
        match &doc.body.sections[0].kind {
            SectionKind::Summary { text } => {
                assert!(text.contains("offensive testing"));
            }
            other => panic!("expected Summary, got {other:?}"),
        }

        // 2. Experience timeline
        match &doc.body.sections[1].kind {
            SectionKind::TimelineEntries { entries } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].primary_heading, "CrowdStrike");
                assert_eq!(
                    entries[0].secondary_heading.as_deref(),
                    Some("Senior Security Analyst")
                );
                assert_eq!(entries[0].start_date, "Apr 2020");
                assert!(entries[0].is_current);
                assert_eq!(entries[0].bullets.len(), 2);
            }
            other => panic!("expected TimelineEntries, got {other:?}"),
        }

        // 3. Skills categorized inline list
        match &doc.body.sections[2].kind {
            SectionKind::CategorizedInlineList { categories } => {
                assert_eq!(categories.len(), 2);
                assert_eq!(categories[0].category_label, "Languages");
                assert_eq!(categories[0].items, vec!["Rust", "Python", "Go"]);
            }
            other => panic!("expected CategorizedInlineList, got {other:?}"),
        }

        // 4. Certifications simple bullet list
        match &doc.body.sections[3].kind {
            SectionKind::SimpleBulletList { items } => {
                assert_eq!(items.len(), 1);
                assert!(items[0].text.contains("OSCP"));
            }
            other => panic!("expected SimpleBulletList, got {other:?}"),
        }
    }

    #[test]
    fn user_can_designate_custom_section_starts_and_lines_underneath_flow_into_section() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Alex Rivera</w:t></w:r></w:p>
    <w:p><w:r><w:t>alex@example.org | Berlin</w:t></w:r></w:p>
    <w:p><w:r><w:t>What I Built Lately</w:t></w:r></w:p>
    <w:p><w:r><w:t>Built an open-source packet analyzer in Rust.</w:t></w:r></w:p>
    <w:p><w:r><w:t>Reduced p99 latency by 45% across edge relays.</w:t></w:r></w:p>
  </w:body>
</w:document>"#;

        let bytes = build_test_docx(xml);
        let mut draft = ImportDraft::from_docx_bytes("custom.docx", &bytes).unwrap();

        // Initially no standard section heading was recognized, so lines are skipped safely.
        assert_eq!(draft.build_body().sections.len(), 0);

        // User marks line #2 ("What I Built Lately") as a Section start!
        draft.set_line_role(
            2,
            ImportLineRole::Section {
                kind: ImportSectionKind::Bullets,
            },
        );

        let doc = draft.build_master_document("Alex Master");
        assert!(doc.pii_header.is_empty());
        assert_eq!(doc.body.sections.len(), 1);
        assert_eq!(doc.body.sections[0].title, "What I Built Lately");
        match &doc.body.sections[0].kind {
            SectionKind::SimpleBulletList { items } => {
                assert_eq!(items.len(), 2);
                assert!(items[0].text.contains("packet analyzer"));
                assert!(items[1].text.contains("45%"));
            }
            other => panic!("expected SimpleBulletList, got {other:?}"),
        }

        // Test merging a wrapped line upward.
        assert!(draft.merge_line_up(4));
        let merged_body = draft.build_body();
        match &merged_body.sections[0].kind {
            SectionKind::SimpleBulletList { items } => {
                assert_eq!(items.len(), 1);
                assert!(items[0].text.contains("packet analyzer"));
                assert!(items[0].text.contains("45%"));
            }
            other => panic!("expected merged SimpleBulletList, got {other:?}"),
        }
    }

    #[test]
    fn roundtrips_vanguardcv_exported_pdf_and_excludes_pii_header() {
        let repo = SqliteRepository::in_memory().unwrap();
        seed::seed(&repo).unwrap();
        let root_id = repo.list_roots().unwrap()[0].id;
        let seeded_doc = repo.load(root_id).unwrap();

        let compiler = TypstCompiler::new().unwrap();
        let pdf_bytes = compiler
            .compile_to_pdf(&seeded_doc.pii_header, &seeded_doc.body)
            .expect("Typst should export seeded CV to PDF");

        let draft = ImportDraft::from_pdf_bytes("VanguardCV_Export.pdf", &pdf_bytes)
            .expect("PDF importer should extract VanguardCV's own PDF cleanly");

        let imported = draft.build_master_document("Reimported Master");
        assert!(
            imported.pii_header.is_empty(),
            "Personal info header must NOT be populated on import"
        );
        // Ensure no personal info (Jordan Blake / email / phone) leaked into imported body sections.
        let body_json = serde_json::to_string(&imported.body).unwrap();
        assert!(
            !body_json.contains("Jordan Blake")
                && !body_json.contains("jordan.blake@protonmail.com")
                && !body_json.contains("847-2991"),
            "PII header lines must remain skipped; leaked into body: {body_json}"
        );

        assert!(
            draft.detected_vanguard_format,
            "Should automatically recognize VanguardCV's own PDF layout"
        );
        // Must detect all 5 seeded sections from the VanguardCV PDF.
        assert_eq!(
            imported.body.sections.len(),
            5,
            "Expected all 5 seeded sections from VanguardCV PDF, got {:?}",
            imported
                .body
                .sections
                .iter()
                .map(|s| (&s.title, s.kind_name()))
                .collect::<Vec<_>>()
        );

        // 1. PROFESSIONAL EXPERIENCE (3 entries with roles, dates, bullets)
        match &imported.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => {
                assert_eq!(entries.len(), 3, "Expected 3 experience entries, got {entries:?}");
                assert_eq!(entries[0].primary_heading, "Independent Bug Bounty Research");
                assert_eq!(entries[0].secondary_heading.as_deref(), Some("Security Researcher"));
                assert!(entries[0].is_current);
                assert_eq!(entries[0].bullets.len(), 3);
                assert_eq!(entries[1].primary_heading, "CrowdStrike");
                assert_eq!(entries[1].bullets.len(), 2);
                assert_eq!(entries[2].primary_heading, "SecureWave Labs");
                assert_eq!(entries[2].bullets.len(), 2);
            }
            other => panic!("expected TimelineEntries for experience, got {other:?}"),
        }

        // 2. EDUCATION (1 entry, 2 bullets)
        match &imported.body.sections[1].kind {
            SectionKind::TimelineEntries { entries } => {
                assert_eq!(entries.len(), 1);
                assert!(entries[0].primary_heading.contains("University of Applied Sciences"));
                assert_eq!(entries[0].bullets.len(), 2);
            }
            other => panic!("expected TimelineEntries for education, got {other:?}"),
        }

        // 3. SKILLS AND OTHERS (3 categorized inline groups)
        match &imported.body.sections[2].kind {
            SectionKind::CategorizedInlineList { categories } => {
                assert_eq!(categories.len(), 3, "Expected 3 skill categories, got {categories:?}");
                assert_eq!(categories[0].category_label, "Soft skills");
                assert_eq!(categories[1].category_label, "Technical skills");
                assert_eq!(categories[2].category_label, "Languages");
            }
            other => panic!("expected CategorizedInlineList for skills, got {other:?}"),
        }

        // 4. HONORS AND AWARDS (3 bullets)
        match &imported.body.sections[3].kind {
            SectionKind::SimpleBulletList { items } => {
                assert_eq!(items.len(), 3, "Expected 3 honors bullets, got {items:?}");
            }
            other => panic!("expected SimpleBulletList for honors, got {other:?}"),
        }

        // 5. TRAINING COURSES (3 bullets)
        match &imported.body.sections[4].kind {
            SectionKind::SimpleBulletList { items } => {
                assert_eq!(items.len(), 3, "Expected 3 training bullets, got {items:?}");
            }
            other => panic!("expected SimpleBulletList for training, got {other:?}"),
        }
    }
}
