//! Pure-Rust `.pdf` text & line extractor for Master CV imports.
//!
//! Uses `pdf-extract` to read text streams, normalizes PDF ligatures and bullet
//! symbols, separates glued section headers / inline bullets (common in Typst
//! and tightly spaced resume PDFs), stitches standalone bullet glyphs, filters
//! standalone page numbers, and rejoins mid-sentence line wraps cleanly.

use std::panic::AssertUnwindSafe;

use anyhow::{Result, anyhow, bail};

use super::classifier::{
    is_known_section_heading_text, line_contains_date_range, split_known_section_heading_prefix,
};
use super::{ExtractedLine, normalize_line_text, strip_leading_bullet_marker};

/// Extracts cleaned, logical lines from raw `.pdf` bytes.
pub fn extract_pdf_lines(bytes: &[u8]) -> Result<(Vec<ExtractedLine>, Vec<String>)> {
    if bytes.len() < 5 || !bytes.starts_with(b"%PDF-") {
        bail!("File does not start with a valid %PDF- header.");
    }

    let raw_text = std::panic::catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(bytes)
    }))
    .map_err(|_| {
        anyhow!(
            "PDF parser encountered an unsupported PDF structure. Try exporting the CV as .docx or re-saving the PDF."
        )
    })?
    .map_err(|err| anyhow!("Could not extract text from PDF: {err}"))?;

    let lines = split_and_clean_pdf_text(&raw_text);
    if lines.is_empty() {
        bail!(
            "No selectable text was found in this PDF (it may be a scanned image). Please use a text-based PDF or .docx file."
        );
    }

    let mut warnings = Vec::new();
    if lines.len() < 4 {
        warnings.push(
            "Very little text was extracted from this PDF. Verify all lines carefully or try importing a .docx copy."
                .to_string(),
        );
    }

    Ok((lines, warnings))
}

/// Converts raw `pdf-extract` output into clean logical `ExtractedLine` items.
pub(crate) fn split_and_clean_pdf_text(raw: &str) -> Vec<ExtractedLine> {
    // 1. Pre-normalize line breaks, filter page numbers, and split glued PDF
    //    structures (e.g. `PROFESSIONAL EXPERIENCEIndependent...` or `Role• Bullet`).
    let mut raw_lines: Vec<String> = Vec::new();
    for part in raw.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let cleaned = normalize_line_text(part);
        if cleaned.is_empty() || is_page_number_artifact(&cleaned) {
            continue;
        }
        expand_glued_pdf_line(&cleaned, &mut raw_lines);
    }

    // 2. Stitch standalone bullet glyphs ("•", "-", "▪", etc.) onto the following line.
    let mut stitched: Vec<(String, bool)> = Vec::with_capacity(raw_lines.len());
    let mut pending_bullet = false;

    for line in raw_lines {
        if is_standalone_bullet_glyph(&line) {
            pending_bullet = true;
            continue;
        }

        let (stripped, had_bullet) = strip_leading_bullet_marker(&line);
        if had_bullet {
            pending_bullet = false;
            if !stripped.is_empty() {
                stitched.push((stripped, true));
            }
        } else if pending_bullet {
            pending_bullet = false;
            stitched.push((line, true));
        } else {
            stitched.push((line, false));
        }
    }

    // 3. Rejoin obvious mid-sentence PDF line wraps for bullet items and long
    //    body paragraphs, without merging section headers, date rows, or new bullets.
    let mut merged: Vec<(String, bool)> = Vec::with_capacity(stitched.len());
    for (text, is_bullet) in stitched {
        let should_merge_with_prev = if is_bullet {
            false
        } else if let Some((prev_text, prev_is_bullet)) = merged.last() {
            should_rejoin_wrapped_line(prev_text, *prev_is_bullet, &text)
        } else {
            false
        };

        if should_merge_with_prev {
            if let Some((prev_text, _)) = merged.last_mut() {
                if prev_text.ends_with('-') && !prev_text.ends_with(" -") {
                    // Preserve hyphen in compound words like "Cross-functional" when the
                    // hyphenated word is wrapped across lines, or join cleanly.
                    prev_text.push_str(&text);
                } else {
                    prev_text.push(' ');
                    prev_text.push_str(&text);
                }
            }
        } else {
            merged.push((text, is_bullet));
        }
    }

    merged
        .into_iter()
        .map(|(text, is_list_item)| {
            ExtractedLine::from_text(text, false, false, is_list_item, false, None, false)
        })
        .collect()
}

/// Splits PDF lines where tight vertical spacing caused a section heading or a
/// bullet item (`•`) to be concatenated onto the previous element on the same line.
fn expand_glued_pdf_line(line: &str, out: &mut Vec<String>) {
    let mut current = line.trim();
    if current.is_empty() {
        return;
    }

    // 1. Peel off a glued known section heading prefix (e.g.
    //    `PROFESSIONAL EXPERIENCEIndependent Bug Bounty...` or
    //    `HONORS AND AWARDS: • Top 1%...`).
    if let Some((heading, rest)) = split_known_section_heading_prefix(current) {
        out.push(heading.to_string());
        current = rest.trim();
        if current.is_empty() {
            return;
        }
    }

    // 2. Split glued inline bullet glyphs `•` (e.g. `Security Researcher• Reported...`
    //    or `...working knowledge)•  Languages: English`).
    //    Avoid splitting short inline skill separators like `Rust • Python • Go` when
    //    the bullet is surrounded by spaces AND the right side isn't a new `Label:` or sentence.
    let mut start = 0usize;
    let bytes = current.as_bytes();
    for (idx, ch) in current.char_indices() {
        if ch == '•' && idx > start {
            let before = current[start..idx].trim();
            let after = current[idx + ch.len_utf8()..].trim();
            let glued_left = idx > 0 && !bytes[idx - 1].is_ascii_whitespace();
            let after_is_category = looks_like_skill_category_line(after);
            let after_is_sentence = after.len() >= 32 || after.ends_with('.');
            let before_ends_clause = before.ends_with(':')
                || before.ends_with('.')
                || before.ends_with(')');

            if !before.is_empty()
                && !after.is_empty()
                && (glued_left || after_is_category || after_is_sentence || before_ends_clause)
            {
                out.push(before.to_string());
                start = idx;
            }
        }
    }

    let tail = current[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
}

fn is_standalone_bullet_glyph(s: &str) -> bool {
    matches!(
        s.trim(),
        "•" | "·" | "▪" | "▫" | "◦" | "‣" | "●" | "○" | "▸" | "▹" | "-" | "–" | "—" | "*" | "\u{F0B7}"
    )
}

fn is_page_number_artifact(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("page ") {
        let compact: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.chars().all(|c| c.is_ascii_digit() || c == '/')
            || rest
                .split_whitespace()
                .all(|tok| tok == "of" || tok.chars().all(|c| c.is_ascii_digit()))
        {
            return true;
        }
    }
    trimmed.len() <= 5
        && trimmed
            .chars()
            .all(|c| c.is_ascii_digit() || c == '/' || c.is_whitespace())
        && trimmed.chars().any(|c| c.is_ascii_digit())
}

fn should_rejoin_wrapped_line(prev: &str, prev_is_bullet: bool, next: &str) -> bool {
    let next_trim = next.trim();
    if next_trim.is_empty() {
        return false;
    }
    if is_known_section_heading_text(next_trim)
        || is_all_caps_heading_candidate(next_trim)
        || line_contains_date_range(next_trim)
        || next_trim.contains(" | ")
        || looks_like_skill_category_line(next_trim)
    {
        return false;
    }
    if is_known_section_heading_text(prev)
        || is_all_caps_heading_candidate(prev)
        || line_contains_date_range(prev)
        || prev.contains(" | ")
    {
        return false;
    }

    let first_char = next_trim.chars().next();
    let starts_lowercase = first_char.is_some_and(|c| c.is_lowercase());
    let prev_ends_sentence = prev.ends_with('.') || prev.ends_with('!') || prev.ends_with('?');
    let prev_ends_continuation = prev.ends_with(',')
        || prev.ends_with(';')
        || prev.ends_with(':')
        || prev.ends_with('-')
        || prev.ends_with('/')
        || prev.ends_with("and")
        || prev.ends_with("or")
        || prev.ends_with("of")
        || prev.ends_with("to")
        || prev.ends_with("in")
        || prev.ends_with("for")
        || prev.ends_with("with");

    if prev_is_bullet {
        if starts_lowercase || prev_ends_continuation {
            return true;
        }
        if !prev_ends_sentence && prev.len() >= 48 {
            return true;
        }
        return false;
    }

    if starts_lowercase && prev.len() >= 35 {
        return true;
    }
    if prev_ends_continuation && prev.len() >= 42 {
        return true;
    }
    if !prev_ends_sentence && prev.len() >= 65 && next_trim.len() >= 15 {
        return true;
    }
    false
}

fn is_all_caps_heading_candidate(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 3 && s.len() <= 52 && letters.iter().all(|c| c.is_uppercase())
}

fn looks_like_skill_category_line(s: &str) -> bool {
    let Some((label, rest)) = s.split_once(':') else {
        return false;
    };
    let label = label.trim();
    let rest = rest.trim();
    !label.is_empty()
        && label.len() <= 40
        && !label.contains("http")
        && !rest.is_empty()
        && label.split_whitespace().count() <= 5
}
