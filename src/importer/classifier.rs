//! Default CV structure classifier and deterministic `SanitizedCvBody` builder.
//!
//! Responsibilities:
//! 1. Automatically classify extracted DOCX/PDF lines into `Skip`, `Section`,
//!    `Entry`, `EntrySubheading`, or `Point`.
//! 2. Recognize VanguardCV's own Classic Serif output (`classic_serif.typ`) as
//!    well as standard resume formats.
//! 3. Strictly exclude the personal information header at the top of the CV so
//!    PII is never parsed or populated into `PiiContactHeader`.
//! 4. Deterministically compile the user-verified line roles into a clean
//!    `SanitizedCvBody` (with `PiiContactHeader::default()` always blank).

use std::sync::OnceLock;

use regex::Regex;
use uuid::Uuid;

use crate::contracts::{
    BulletItem, CvSection, SanitizedCvBody, SectionKind, SkillCategoryItem, TimelineEntry,
};

use super::{ExtractedLine, ImportLine, ImportLineRole, ImportSectionKind};

fn date_range_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Matches:
        // - "Mar 2021 - (Currently)" / "Apr 2020 – Feb 2021" / "Jan 2020 to Present"
        // - "04/2020 - 02/2021" / "2020-04 - Present"
        // - "2019 - 2023" / "2021 – Present" / "(2018 - 2022)"
        let month = r"(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)";
        let point = format!(r"(?:{month}\.?\s+\d{{4}}|\d{{1,2}}/\d{{4}}|\d{{4}}-\d{{2}}|\d{{4}})");
        let end_point = format!(
            r"(?:\(?\s*(?:Currently|Current|Present|Now|Ongoing|Today)\s*\)?|{point})"
        );
        let pattern = format!(
            r"(?i)\(?\s*(?P<start>{point})\s*(?:[-–—]|\bto\b)\s*(?P<end>{end_point})\s*\)?"
        );
        Regex::new(&pattern).expect("valid date range regex")
    })
}

fn standalone_end_year_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Matches a trailing month+year or 4-digit graduation/project year at the
        // end of an entry header line, e.g. "B.S. Computer Science   May 2020" or "MIT   2022".
        let month = r"(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)";
        let pattern = format!(
            r"(?i)(?:\s{{2,}}|\s*[|·•,]\s*|\s+\()(?P<date>(?:{month}\.?\s+)?(?:19|20)\d{{2}})\)?\s*$"
        );
        Regex::new(&pattern).expect("valid standalone date regex")
    })
}

fn email_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b[A-Z0-9._%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,}\b").expect("valid email regex")
    })
}

fn phone_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:\+?\d{1,3}[\s.\-]?)?(?:\(?\d{3}\)?[\s.\-])\d{3}[\s.\-]\d{4}")
            .expect("valid phone regex")
    })
}

fn url_or_social_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:https?://|www\.|linkedin\.com/|github\.com/|gitlab\.com/|x\.com/|twitter\.com/)")
            .expect("valid url regex")
    })
}

/// Checks if a line contains a recognizable CV date range (e.g. `Mar 2021 - (Currently)`).
pub(crate) fn line_contains_date_range(text: &str) -> bool {
    date_range_regex().is_match(text)
}

/// Returns the canonical `ImportSectionKind` if `text` matches a known CV section title.
pub fn match_known_section_heading(text: &str) -> Option<ImportSectionKind> {
    let cleaned = normalize_heading_key(text);
    if cleaned.is_empty() || cleaned.len() > 48 {
        return None;
    }

    match cleaned.as_str() {
        // Summary / Profile
        "summary"
        | "professional summary"
        | "executive summary"
        | "career summary"
        | "profile"
        | "professional profile"
        | "career profile"
        | "objective"
        | "career objective"
        | "about"
        | "about me"
        | "overview" => Some(ImportSectionKind::Summary),

        // Timeline (Experience, Education, Projects, Leadership)
        "experience"
        | "work experience"
        | "professional experience"
        | "relevant experience"
        | "employment"
        | "employment history"
        | "work history"
        | "career history"
        | "internships"
        | "internship experience"
        | "education"
        | "academic background"
        | "academic qualifications"
        | "projects"
        | "key projects"
        | "selected projects"
        | "personal projects"
        | "technical projects"
        | "research"
        | "research experience"
        | "volunteering"
        | "volunteer experience"
        | "leadership"
        | "leadership experience"
        | "extracurricular activities" => Some(ImportSectionKind::Timeline),

        // Categorized Skills
        "skills"
        | "technical skills"
        | "skills and others"
        | "skills & others"
        | "core competencies"
        | "competencies"
        | "technologies"
        | "technical stack"
        | "tech stack"
        | "skills and technologies"
        | "skills & technologies"
        | "languages and tools"
        | "languages & tools"
        | "tools and technologies"
        | "tools & technologies"
        | "areas of expertise"
        | "expertise" => Some(ImportSectionKind::Skills),

        // Simple Bullet Lists
        "certifications"
        | "certificates"
        | "licenses and certifications"
        | "licenses & certifications"
        | "honors"
        | "awards"
        | "honors and awards"
        | "honors & awards"
        | "awards and honors"
        | "awards & honors"
        | "achievements"
        | "key achievements"
        | "accomplishments"
        | "training"
        | "training courses"
        | "courses"
        | "coursework"
        | "relevant coursework"
        | "publications"
        | "patents"
        | "languages"
        | "interests"
        | "additional information"
        | "activities" => Some(ImportSectionKind::Bullets),

        _ => None,
    }
}

pub(crate) fn is_known_section_heading_text(text: &str) -> bool {
    match_known_section_heading(text).is_some()
}

/// If `line` starts with a known uppercase/canonical section heading glued directly
/// onto the next line's content (e.g. `PROFESSIONAL EXPERIENCEIndependent...` or
/// `HONORS AND AWARDS: • Top 1%...`), returns `(heading, remainder)`.
pub(crate) fn split_known_section_heading_prefix(line: &str) -> Option<(&str, &str)> {
    const PREFIXES: &[&str] = &[
        "PROFESSIONAL EXPERIENCE",
        "LICENSES AND CERTIFICATIONS",
        "LICENSES & CERTIFICATIONS",
        "SKILLS AND TECHNOLOGIES",
        "SKILLS & TECHNOLOGIES",
        "ADDITIONAL INFORMATION",
        "EXTRACURRICULAR ACTIVITIES",
        "ACADEMIC QUALIFICATIONS",
        "PROFESSIONAL SUMMARY",
        "PROFESSIONAL PROFILE",
        "RELEVANT EXPERIENCE",
        "VOLUNTEER EXPERIENCE",
        "LEADERSHIP EXPERIENCE",
        "INTERNSHIP EXPERIENCE",
        "RESEARCH EXPERIENCE",
        "TOOLS AND TECHNOLOGIES",
        "TOOLS & TECHNOLOGIES",
        "LANGUAGES AND TOOLS",
        "LANGUAGES & TOOLS",
        "RELEVANT COURSEWORK",
        "EMPLOYMENT HISTORY",
        "ACADEMIC BACKGROUND",
        "SKILLS AND OTHERS",
        "SKILLS & OTHERS",
        "HONORS AND AWARDS",
        "HONORS & AWARDS",
        "AWARDS AND HONORS",
        "AWARDS & HONORS",
        "CORE COMPETENCIES",
        "EXECUTIVE SUMMARY",
        "AREAS OF EXPERTISE",
        "TECHNICAL SKILLS",
        "TECHNICAL PROJECTS",
        "SELECTED PROJECTS",
        "PERSONAL PROJECTS",
        "TRAINING COURSES",
        "KEY ACHIEVEMENTS",
        "CAREER OBJECTIVE",
        "WORK EXPERIENCE",
        "TECHNICAL STACK",
        "CAREER SUMMARY",
        "CAREER PROFILE",
        "CAREER HISTORY",
        "CERTIFICATIONS",
        "ACCOMPLISHMENTS",
        "WORK HISTORY",
        "KEY PROJECTS",
        "VOLUNTEERING",
        "PUBLICATIONS",
        "ACHIEVEMENTS",
        "COMPETENCIES",
        "TECHNOLOGIES",
        "CERTIFICATES",
        "INTERNSHIPS",
        "EXPERIENCE",
        "EMPLOYMENT",
        "LEADERSHIP",
        "TECH STACK",
        "COURSEWORK",
        "EDUCATION",
        "OBJECTIVE",
        "EXPERTISE",
        "LANGUAGES",
        "INTERESTS",
        "ACTIVITIES",
        "PROJECTS",
        "RESEARCH",
        "TRAINING",
        "OVERVIEW",
        "ABOUT ME",
        "SUMMARY",
        "PROFILE",
        "COURSES",
        "PATENTS",
        "SKILLS",
        "HONORS",
        "AWARDS",
    ];

    let trimmed = line.trim();
    for &prefix in PREFIXES {
        if trimmed.len() <= prefix.len() || !trimmed.is_char_boundary(prefix.len()) {
            continue;
        }
        // Match exact uppercase prefix (typical of VanguardCV / resume PDFs) OR
        // case-insensitive prefix followed explicitly by `:` or `•`.
        let head = &trimmed[..prefix.len()];
        let rest = &trimmed[prefix.len()..];
        let exact_upper = head == prefix;
        let ci_with_delim = head.eq_ignore_ascii_case(prefix)
            && (rest.starts_with(':') || rest.trim_start().starts_with('•'));
        if !exact_upper && !ci_with_delim {
            continue;
        }
        // Make sure we didn't split mid-word in lowercase/same-case text:
        // Valid boundaries: `:`, `•`, whitespace, or transition from UPPERCASE to TitleCase (`EXPERIENCEIndependent`).
        let first_rest = rest.chars().next()?;
        let valid_boundary = first_rest == ':'
            || first_rest == '•'
            || first_rest.is_whitespace()
            || (exact_upper
                && rest.chars().nth(1).is_some_and(|c| c.is_lowercase()));
        if !valid_boundary {
            continue;
        }
        let remainder = rest.trim_start_matches(':').trim_start();
        if !remainder.is_empty() {
            return Some((head, remainder));
        }
    }
    None
}

/// Infers a sensible `ImportSectionKind` for ANY user-designated section title.
pub fn infer_section_kind_for_title(title: &str) -> ImportSectionKind {
    if let Some(exact) = match_known_section_heading(title) {
        return exact;
    }
    let key = normalize_heading_key(title);
    if key.contains("summary")
        || key.contains("profile")
        || key.contains("objective")
        || key.contains("about")
    {
        return ImportSectionKind::Summary;
    }
    if key.contains("skill")
        || key.contains("tech")
        || key.contains("competenc")
        || key.contains("tool")
        || key.contains("stack")
    {
        return ImportSectionKind::Skills;
    }
    if key.contains("experience")
        || key.contains("employment")
        || key.contains("education")
        || key.contains("project")
        || key.contains("work")
        || key.contains("career")
        || key.contains("intern")
        || key.contains("research")
    {
        return ImportSectionKind::Timeline;
    }
    ImportSectionKind::Bullets
}

fn normalize_heading_key(text: &str) -> String {
    text.trim()
        .trim_end_matches(':')
        .trim_matches(|c: char| matches!(c, '—' | '–' | '-' | '•' | '*' | '_'))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Detects whether a line looks like contact/PII information (email, phone, URL, contact pipe bar).
fn looks_like_contact_pii(text: &str) -> bool {
    let t = text.trim();
    if email_regex().is_match(t) || phone_regex().is_match(t) || url_or_social_regex().is_match(t) {
        return true;
    }
    // Pipe-separated contact lines such as "Remote — EU/US | linkedin.com/in/..."
    if t.matches('|').count() >= 2 && t.len() <= 140 {
        return true;
    }
    false
}

/// Heuristic: does this extracted line look like an unrecognized/custom section heading?
fn looks_like_custom_section_heading(line: &ExtractedLine, idx: usize) -> bool {
    let t = line.text.trim();
    if t.len() < 3 || t.len() > 50 {
        return false;
    }
    if line.is_list_item || line.is_table_row {
        return false;
    }
    if looks_like_contact_pii(t) || line_contains_date_range(t) {
        return false;
    }
    if t.ends_with('.') || t.contains(',') || t.contains(':') {
        return false;
    }
    // Don't mistake the very first line (usually the candidate's name in big/bold text)
    // for a section heading unless it's a known section name (already checked earlier).
    if idx == 0 {
        return false;
    }
    let word_count = t.split_whitespace().count();
    if word_count > 6 {
        return false;
    }

    if line.is_heading {
        return true;
    }
    if line.has_bottom_border && (line.is_bold || line.is_all_caps) {
        return true;
    }
    if line.is_all_caps && t.chars().filter(|c| c.is_alphabetic()).count() >= 4 {
        // Avoid misclassifying 1-word tech acronyms inside a body as headings if short,
        // unless it's bold or at least 5 letters.
        return line.is_bold || t.len() >= 5;
    }
    false
}

/// Classifies extracted lines using the default parser, returning:
/// `(classified_lines, detected_vanguard_format)`.
pub fn classify_lines(raw_lines: Vec<ExtractedLine>) -> (Vec<ImportLine>, bool) {
    // 1. Detect VanguardCV's own Classic Serif output cues:
    //    - Uppercase known section headings (e.g., "PROFESSIONAL EXPERIENCE")
    //    - "(Currently)" marker or "• Label:" skills format or pipe-separated contact header
    let has_currently_marker = raw_lines
        .iter()
        .any(|l| l.text.contains("(Currently)") || l.text.contains("- (Currently)"));
    let uppercase_known_sections = raw_lines
        .iter()
        .filter(|l| l.is_all_caps && match_known_section_heading(&l.text).is_some())
        .count();
    let has_pipe_contact_top = raw_lines
        .iter()
        .take(5)
        .any(|l| l.text.contains(" | ") && looks_like_contact_pii(&l.text));

    let detected_vanguard_format = uppercase_known_sections >= 2
        && (has_currently_marker || has_pipe_contact_top);

    // 2. First pass: locate section start indices so we cleanly isolate the top
    //    personal info header before the first section.
    let mut section_starts: Vec<(usize, ImportSectionKind)> = Vec::new();
    for (idx, line) in raw_lines.iter().enumerate() {
        if let Some(kind) = match_known_section_heading(&line.text) {
            section_starts.push((idx, kind));
        } else if looks_like_custom_section_heading(line, idx)
            && !looks_like_top_name_or_headline(&raw_lines, idx)
        {
            let kind = infer_section_kind_for_title(&line.text);
            section_starts.push((idx, kind));
        }
    }

    let first_section_idx = section_starts.first().map(|(i, _)| *i);

    // Refine Skills vs Bullets when a Skills section has no `Label: items` rows.
    for s_pos in 0..section_starts.len() {
        let (start_idx, kind) = section_starts[s_pos];
        if kind == ImportSectionKind::Skills {
            let end_idx = section_starts
                .get(s_pos + 1)
                .map(|(i, _)| *i)
                .unwrap_or(raw_lines.len());
            let body_slice = &raw_lines[(start_idx + 1).min(raw_lines.len())..end_idx];
            let has_colon_categories = body_slice.iter().any(|l| split_skill_category(&l.text).is_some());
            if !body_slice.is_empty() && !has_colon_categories {
                section_starts[s_pos].1 = ImportSectionKind::Bullets;
            }
        }
    }

    // 3. Second pass: assign every line an `ImportLineRole`.
    let mut out = Vec::with_capacity(raw_lines.len());
    let mut current_section_kind: Option<ImportSectionKind> = None;
    let mut prev_was_entry = false;

    for (idx, raw) in raw_lines.into_iter().enumerate() {
        if let Some((_, kind)) = section_starts.iter().find(|(s_idx, _)| *s_idx == idx) {
            current_section_kind = Some(*kind);
            prev_was_entry = false;
            let role = ImportLineRole::Section { kind: *kind };
            out.push(ImportLine {
                id: idx,
                text: clean_section_title(&raw.text),
                raw,
                role,
                auto_role: role,
                is_likely_pii_header: false,
                user_modified: false,
            });
            continue;
        }

        // Everything before the first section heading is treated as Personal Info Header
        // and skipped automatically (never parsed into PiiContactHeader).
        let is_before_first_section = first_section_idx.is_some_and(|first| idx < first);
        let is_top_pii_without_sections =
            first_section_idx.is_none() && (idx < 4 || looks_like_contact_pii(&raw.text));

        if is_before_first_section || is_top_pii_without_sections {
            let role = ImportLineRole::Skip;
            out.push(ImportLine {
                id: idx,
                text: raw.text.clone(),
                raw,
                role,
                auto_role: role,
                is_likely_pii_header: true,
                user_modified: false,
            });
            continue;
        }

        let role = match current_section_kind {
            None => ImportLineRole::Skip,
            Some(ImportSectionKind::Summary)
            | Some(ImportSectionKind::Skills)
            | Some(ImportSectionKind::Bullets) => {
                prev_was_entry = false;
                ImportLineRole::Point
            }
            Some(ImportSectionKind::Timeline) => {
                let role = classify_timeline_line(&raw, prev_was_entry);
                prev_was_entry = role == ImportLineRole::Entry;
                role
            }
        };

        out.push(ImportLine {
            id: idx,
            text: raw.text.clone(),
            raw,
            role,
            auto_role: role,
            is_likely_pii_header: false,
            user_modified: false,
        });
    }

    (out, detected_vanguard_format)
}

/// Prevents the candidate's name or headline at the very top of the CV (above
/// contact details like email/phone/LinkedIn) from being misclassified as a custom section.
fn looks_like_top_name_or_headline(lines: &[ExtractedLine], idx: usize) -> bool {
    if idx > 2 {
        return false;
    }
    // If there is a contact/PII line shortly below `idx` (within the top 6 lines),
    // `idx` is part of the personal info header.
    lines
        .iter()
        .enumerate()
        .skip(idx + 1)
        .take(5)
        .any(|(_, l)| looks_like_contact_pii(&l.text))
}

fn classify_timeline_line(line: &ExtractedLine, prev_was_entry: bool) -> ImportLineRole {
    let t = line.text.trim();
    if line.is_list_item {
        return ImportLineRole::Point;
    }
    if line_contains_date_range(t) || standalone_end_year_regex().is_match(t) {
        return ImportLineRole::Entry;
    }
    // Line immediately following an Entry that looks like a job title / degree
    // (e.g. VanguardCV's `Security Researcher` directly below `Independent Bug Bounty Research   Mar 2021 - (Currently)`).
    if prev_was_entry
        && t.len() <= 92
        && !t.ends_with('.')
        && (line.is_bold || t.split_whitespace().count() <= 11)
    {
        return ImportLineRole::EntrySubheading;
    }
    // A short bold or table line without sentence punctuation inside a Timeline section
    // is very likely an entry header (even if dates were omitted).
    if (line.is_bold || line.is_table_row) && t.len() <= 85 && !t.ends_with('.') {
        return ImportLineRole::Entry;
    }
    ImportLineRole::Point
}

/// Deterministically builds a `SanitizedCvBody` from the current (auto or user-edited) lines.
pub fn build_body_from_lines(lines: &[ImportLine]) -> SanitizedCvBody {
    let mut sections: Vec<CvSection> = Vec::new();

    for line in lines {
        let text = line.text.trim();
        if text.is_empty() {
            continue;
        }

        match line.role {
            ImportLineRole::Skip => continue,
            ImportLineRole::Section { kind } => {
                let title = clean_section_title(text);
                let section_kind = match kind {
                    ImportSectionKind::Summary => SectionKind::Summary {
                        text: String::new(),
                    },
                    ImportSectionKind::Timeline => SectionKind::TimelineEntries {
                        entries: Vec::new(),
                    },
                    ImportSectionKind::Skills => SectionKind::CategorizedInlineList {
                        categories: Vec::new(),
                    },
                    ImportSectionKind::Bullets => SectionKind::SimpleBulletList {
                        items: Vec::new(),
                    },
                };
                sections.push(CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title,
                    enabled: true,
                    kind: section_kind,
                });
            }
            ImportLineRole::Entry => {
                ensure_active_section(&mut sections, ImportSectionKind::Timeline, "Experience");
                if let Some(sec) = sections.last_mut() {
                    match &mut sec.kind {
                        SectionKind::TimelineEntries { entries } => {
                            entries.push(parse_timeline_entry_header(text));
                        }
                        SectionKind::SimpleBulletList { items } => {
                            items.push(BulletItem::new(text));
                        }
                        SectionKind::CategorizedInlineList { categories } => {
                            push_skill_line(categories, text);
                        }
                        SectionKind::Summary { text: sum } => {
                            append_summary_line(sum, text);
                        }
                    }
                }
            }
            ImportLineRole::EntrySubheading => {
                if let Some(sec) = sections.last_mut() {
                    match &mut sec.kind {
                        SectionKind::TimelineEntries { entries } => {
                            if let Some(last_entry) = entries.last_mut() {
                                match &mut last_entry.secondary_heading {
                                    Some(existing) if !existing.trim().is_empty() => {
                                        existing.push_str(" — ");
                                        existing.push_str(text);
                                    }
                                    _ => {
                                        last_entry.secondary_heading = Some(text.to_string());
                                    }
                                }
                            } else {
                                entries.push(parse_timeline_entry_header(text));
                            }
                        }
                        SectionKind::SimpleBulletList { items } => {
                            items.push(BulletItem::new(text));
                        }
                        SectionKind::CategorizedInlineList { categories } => {
                            push_skill_line(categories, text);
                        }
                        SectionKind::Summary { text: sum } => {
                            append_summary_line(sum, text);
                        }
                    }
                }
            }
            ImportLineRole::Point => {
                if sections.is_empty() {
                    ensure_active_section(&mut sections, ImportSectionKind::Bullets, "Highlights");
                }
                if let Some(sec) = sections.last_mut() {
                    match &mut sec.kind {
                        SectionKind::Summary { text: sum } => {
                            append_summary_line(sum, text);
                        }
                        SectionKind::TimelineEntries { entries } => {
                            if entries.is_empty() {
                                entries.push(TimelineEntry::new("General", ""));
                            }
                            if let Some(last_entry) = entries.last_mut() {
                                last_entry.bullets.push(BulletItem::new(text));
                            }
                        }
                        SectionKind::CategorizedInlineList { categories } => {
                            push_skill_line(categories, text);
                        }
                        SectionKind::SimpleBulletList { items } => {
                            items.push(BulletItem::new(text));
                        }
                    }
                }
            }
        }
    }

    SanitizedCvBody { sections }
}

fn ensure_active_section(
    sections: &mut Vec<CvSection>,
    default_kind: ImportSectionKind,
    default_title: &str,
) {
    if sections.is_empty() {
        let kind = match default_kind {
            ImportSectionKind::Summary => SectionKind::Summary {
                text: String::new(),
            },
            ImportSectionKind::Timeline => SectionKind::TimelineEntries {
                entries: Vec::new(),
            },
            ImportSectionKind::Skills => SectionKind::CategorizedInlineList {
                categories: Vec::new(),
            },
            ImportSectionKind::Bullets => SectionKind::SimpleBulletList {
                items: Vec::new(),
            },
        };
        sections.push(CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: default_title.to_string(),
            enabled: true,
            kind,
        });
    }
}

fn clean_section_title(raw: &str) -> String {
    raw.trim()
        .trim_end_matches(':')
        .trim()
        .to_string()
}

fn append_summary_line(summary: &mut String, line: &str) {
    if !summary.is_empty() {
        summary.push(' ');
    }
    summary.push_str(line.trim());
}

fn push_skill_line(categories: &mut Vec<SkillCategoryItem>, line: &str) {
    if let Some((label, items, sep)) = split_skill_category(line) {
        categories.push(SkillCategoryItem {
            id: Uuid::new_v4(),
            origin_item_id: None,
            enabled: true,
            category_label: label,
            items,
            separator: sep,
        });
        return;
    }

    // Fallback when a skill line doesn't have a `Label:` prefix: split by comma/pipe/dash
    // or add as a single item inside a "General" category.
    let (parsed_items, sep) = split_skill_items(line);
    if let Some(existing) = categories
        .iter_mut()
        .find(|c| c.category_label.eq_ignore_ascii_case("General"))
    {
        existing.items.extend(parsed_items);
    } else {
        categories.push(SkillCategoryItem {
            id: Uuid::new_v4(),
            origin_item_id: None,
            enabled: true,
            category_label: "General".to_string(),
            items: parsed_items,
            separator: sep,
        });
    }
}

/// Splits `"Security & AppSec: Burp Suite, OWASP, Python"` into `("Security & AppSec", vec![...], ", ")`.
pub(crate) fn split_skill_category(line: &str) -> Option<(String, Vec<String>, String)> {
    let (raw_label, raw_items) = line.split_once(':')?;
    let label = raw_label
        .trim()
        .trim_start_matches(['•', '-', '*', '▪', '◦'])
        .trim();
    let rest = raw_items.trim();
    if label.is_empty() || label.len() > 48 || rest.is_empty() || label.contains("http") {
        return None;
    }
    let (items, sep) = split_skill_items(rest);
    if items.is_empty() {
        return None;
    }
    Some((label.to_string(), items, sep))
}

fn split_skill_items(raw: &str) -> (Vec<String>, String) {
    let (delimiter, out_sep) = if raw.contains(" • ") {
        (" • ", " • ")
    } else if raw.contains(" | ") {
        (" | ", " | ")
    } else if raw.contains(" - ") && !raw.contains(',') {
        (" - ", " - ")
    } else if raw.contains(';') {
        (";", ", ")
    } else {
        (",", ", ")
    };
    let items: Vec<String> = raw
        .split(delimiter)
        .map(|s| s.trim().trim_end_matches('.').trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        (vec![raw.trim().to_string()], out_sep.to_string())
    } else {
        (items, out_sep.to_string())
    }
}

/// Parses an entry header line into a `TimelineEntry` (`primary_heading`,
/// optional `secondary_heading`, `start_date`, `end_date`, `is_current`).
pub(crate) fn parse_timeline_entry_header(line: &str) -> TimelineEntry {
    let trimmed = line.trim();
    let mut start_date = String::new();
    let mut end_date: Option<String> = None;
    let mut is_current = false;
    let mut remaining = trimmed.to_string();

    if let Some(caps) = date_range_regex().captures(trimmed) {
        if let Some(m_start) = caps.name("start") {
            start_date = m_start.as_str().trim().to_string();
        }
        if let Some(m_end) = caps.name("end") {
            let end_raw = m_end.as_str().trim();
            let end_clean = end_raw
                .trim_matches(|c: char| c == '(' || c == ')' || c.is_whitespace())
                .to_ascii_lowercase();
            if matches!(
                end_clean.as_str(),
                "currently" | "current" | "present" | "now" | "ongoing" | "today"
            ) {
                is_current = true;
            } else {
                end_date = Some(end_raw.to_string());
            }
        }
        if let Some(full) = caps.get(0) {
            let before = trimmed[..full.start()].trim();
            let after = trimmed[full.end()..].trim();
            remaining = match (before.is_empty(), after.is_empty()) {
                (false, true) => before.to_string(),
                (true, false) => after.to_string(),
                (false, false) => format!("{before} | {after}"),
                (true, true) => String::new(),
            };
        }
    } else if let Some(caps) = standalone_end_year_regex().captures(trimmed) {
        if let Some(m_date) = caps.name("date") {
            start_date = m_date.as_str().trim().to_string();
        }
        if let Some(full) = caps.get(0) {
            remaining = trimmed[..full.start()].trim().to_string();
        }
    }

    let cleaned_heading = remaining
        .trim()
        .trim_matches(|c: char| matches!(c, '|' | '—' | '–' | '-' | '·' | '•' | ','))
        .trim()
        .to_string();

    // If the remaining heading contains a clean separator like `Company — Role` or
    // `Company | Role`, split into primary + secondary headings.
    let (primary, secondary) = split_primary_and_secondary(&cleaned_heading);
    let final_primary = if primary.is_empty() {
        trimmed.to_string()
    } else {
        primary
    };

    let mut entry = TimelineEntry::new(final_primary, start_date);
    entry.secondary_heading = secondary;
    entry.end_date = end_date;
    entry.is_current = is_current;
    entry
}

fn split_primary_and_secondary(heading: &str) -> (String, Option<String>) {
    for sep in [" | ", " — ", " – ", " - "] {
        if let Some((left, right)) = heading.split_once(sep) {
            let left = left.trim();
            let right = right.trim();
            if !left.is_empty() && !right.is_empty() {
                return (left.to_string(), Some(right.to_string()));
            }
        }
    }
    (heading.to_string(), None)
}
