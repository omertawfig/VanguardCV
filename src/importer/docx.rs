//! Pure-Rust `.docx` (OOXML) line & formatting extractor.
//!
//! Reads `word/document.xml` (and `word/styles.xml` when present) from a `.docx`
//! ZIP archive using `zip` + `roxmltree`, preserving paragraph order, tables,
//! heading styles, bold runs, list items (`w:numPr`), font sizes, and bottom
//! paragraph borders (commonly used for section dividers in resumes).

use std::collections::HashSet;
use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail};
use roxmltree::{Document, Node};
use zip::ZipArchive;

use super::{ExtractedLine, normalize_line_text, strip_leading_bullet_marker};

/// Safety limit for uncompressed XML inside a `.docx` archive (25 MiB).
const MAX_XML_BYTES: u64 = 25 * 1024 * 1024;

/// Extracts ordered, formatting-annotated lines from `.docx` bytes.
pub fn extract_docx_lines(bytes: &[u8]) -> Result<Vec<ExtractedLine>> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .context("File is not a valid .docx archive (could not open ZIP container)")?;

    let heading_style_ids = read_heading_style_ids(&mut archive);

    let document_xml = read_zip_entry_utf8(&mut archive, "word/document.xml")?
        .context("Invalid .docx file: missing word/document.xml")?;

    let xml = Document::parse(&document_xml)
        .context("Failed to parse XML inside word/document.xml")?;

    let body = xml
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "body")
        .context("Invalid .docx file: word/document.xml has no <w:body> element")?;

    let mut out = Vec::new();
    for child in body.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "p" => extract_paragraph(child, &heading_style_ids, false, &mut out),
            "tbl" => extract_table(child, &heading_style_ids, &mut out),
            "sdt" => {
                // Structured document tags (content controls) may wrap paragraphs/tables.
                if let Some(content) = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "sdtContent")
                {
                    for inner in content.children().filter(|n| n.is_element()) {
                        match inner.tag_name().name() {
                            "p" => extract_paragraph(inner, &heading_style_ids, false, &mut out),
                            "tbl" => extract_table(inner, &heading_style_ids, &mut out),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if out.is_empty() {
        bail!("No readable text paragraphs were found in this .docx file.");
    }

    Ok(out)
}

fn read_zip_entry_utf8(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<String>> {
    let file = match archive.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("Failed reading {name} from .docx")),
    };

    if file.size() > MAX_XML_BYTES {
        bail!("{name} exceeds the 25 MiB safety limit for CV imports.");
    }

    let mut limited = file.take(MAX_XML_BYTES + 1);
    let mut buf = Vec::new();
    limited
        .read_to_end(&mut buf)
        .with_context(|| format!("Failed decompressing {name} from .docx"))?;
    if buf.len() as u64 > MAX_XML_BYTES {
        bail!("{name} exceeds the 25 MiB safety limit for CV imports.");
    }

    let text = String::from_utf8(buf)
        .with_context(|| format!("{name} inside .docx is not valid UTF-8"))?;
    Ok(Some(text))
}

/// Inspects `word/styles.xml` for style IDs whose display name or outline level
/// indicates a heading (e.g. `Heading1`, `SectionHeader`, `Title`).
fn read_heading_style_ids(archive: &mut ZipArchive<Cursor<&[u8]>>) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(Some(xml_str)) = read_zip_entry_utf8(archive, "word/styles.xml") else {
        return ids;
    };
    let Ok(doc) = Document::parse(&xml_str) else {
        return ids;
    };

    for style in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "style")
    {
        let Some(style_id) = attr_by_local_name(style, "styleId") else {
            continue;
        };
        let lower_id = style_id.to_ascii_lowercase();
        let name_val = style
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "name")
            .and_then(|n| attr_by_local_name(n, "val"))
            .unwrap_or("")
            .to_ascii_lowercase();
        let has_outline_lvl = style
            .descendants()
            .any(|n| n.is_element() && n.tag_name().name() == "outlineLvl");

        if has_outline_lvl
            || lower_id.contains("heading")
            || name_val.contains("heading")
            || lower_id.contains("section")
            || name_val.contains("section")
        {
            ids.insert(style_id.to_string());
        }
    }

    ids
}

fn extract_table(
    tbl: Node<'_, '_>,
    heading_style_ids: &HashSet<String>,
    out: &mut Vec<ExtractedLine>,
) {
    for tr in tbl
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "tr")
    {
        let cells: Vec<Node<'_, '_>> = tr
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "tc")
            .collect();

        if cells.is_empty() {
            continue;
        }

        // Extract paragraphs per cell first to decide whether this row is a
        // single-line paired header (e.g. `Company | Mar 2021 - Present`) or a
        // multi-paragraph layout table (e.g. two-column CV template).
        let mut cell_lines: Vec<Vec<ExtractedLine>> = Vec::with_capacity(cells.len());
        for cell in &cells {
            let mut lines = Vec::new();
            for child in cell.children().filter(|n| n.is_element()) {
                match child.tag_name().name() {
                    "p" => extract_paragraph(child, heading_style_ids, true, &mut lines),
                    "tbl" => extract_table(child, heading_style_ids, &mut lines),
                    _ => {}
                }
            }
            if !lines.is_empty() {
                cell_lines.push(lines);
            }
        }

        if cell_lines.is_empty() {
            continue;
        }

        // Compact 2-cell or 3-cell single-line row → join with `   ` so
        // `Company | Mar 2021 - Present` or `Category | Skill1, Skill2` stays on one line.
        let all_single_line = cell_lines.len() >= 2
            && cell_lines.len() <= 3
            && cell_lines.iter().all(|c| c.len() == 1);

        if all_single_line {
            let joined_text = cell_lines
                .iter()
                .map(|c| c[0].text.as_str())
                .collect::<Vec<_>>()
                .join("   ");
            let is_bold = cell_lines[0][0].is_bold;
            let is_heading = cell_lines[0][0].is_heading;
            let is_list_item = cell_lines[0][0].is_list_item;
            let has_bottom_border = cell_lines.iter().any(|c| c[0].has_bottom_border);
            let font_size_pt = cell_lines
                .iter()
                .filter_map(|c| c[0].font_size_pt)
                .reduce(f32::max);

            out.push(ExtractedLine::from_text(
                joined_text,
                is_bold,
                is_heading,
                is_list_item,
                has_bottom_border,
                font_size_pt,
                true,
            ));
        } else {
            for lines in cell_lines {
                out.extend(lines);
            }
        }
    }
}

fn extract_paragraph(
    p: Node<'_, '_>,
    heading_style_ids: &HashSet<String>,
    in_table: bool,
    out: &mut Vec<ExtractedLine>,
) {
    let p_pr = p
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "pPr");

    let mut is_heading = false;
    let mut is_list_item = false;
    let mut has_bottom_border = false;
    let mut para_bold = false;
    let mut para_font_size: Option<f32> = None;

    if let Some(pr) = p_pr {
        if let Some(p_style) = pr
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "pStyle")
            && let Some(val) = attr_by_local_name(p_style, "val")
        {
            let lower = val.to_ascii_lowercase();
            if heading_style_ids.contains(val)
                || lower.starts_with("heading")
                || lower.contains("section")
                || lower == "title"
            {
                is_heading = true;
            }
        }
        if pr
            .children()
            .any(|n| n.is_element() && n.tag_name().name() == "numPr")
        {
            is_list_item = true;
        }
        if let Some(pbdr) = pr
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "pBdr")
            && let Some(bottom) = pbdr
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "bottom")
        {
            let val = attr_by_local_name(bottom, "val").unwrap_or("");
            if !val.eq_ignore_ascii_case("none") && !val.eq_ignore_ascii_case("nil") {
                has_bottom_border = true;
            }
        }
        if let Some(rpr) = pr
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "rPr")
        {
            para_bold = is_rpr_bold(rpr);
            para_font_size = rpr_font_size_pt(rpr);
        }
    }

    // Split on soft line breaks (<w:br/> / <w:cr/>) inside a paragraph while
    // keeping run formatting stats per logical line.
    #[derive(Default)]
    struct Segment {
        text: String,
        bold_chars: usize,
        total_chars: usize,
        max_font_pt: Option<f32>,
    }

    let mut segments: Vec<Segment> = vec![Segment::default()];

    let walk_runs = |container: Node<'_, '_>, segments: &mut Vec<Segment>| {
        for child in container.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "r" => {
                    collect_run(child, para_bold, para_font_size, segments);
                }
                "hyperlink" | "ins" | "smartTag" | "sdt" => {
                    for inner in child
                        .descendants()
                        .filter(|n| n.is_element() && n.tag_name().name() == "r")
                    {
                        collect_run(inner, para_bold, para_font_size, segments);
                    }
                }
                _ => {}
            }
        }
    };

    fn collect_run(
        run: Node<'_, '_>,
        para_bold: bool,
        para_font_size: Option<f32>,
        segments: &mut Vec<Segment>,
    ) {
        let rpr = run
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "rPr");
        let run_bold = rpr.map(is_rpr_bold).unwrap_or(para_bold);
        let run_size = rpr.and_then(rpr_font_size_pt).or(para_font_size);

        for node in run.children().filter(|n| n.is_element()) {
            match node.tag_name().name() {
                "t" => {
                    if let Some(text) = node.text() {
                        let seg = segments.last_mut().expect("at least one segment");
                        seg.text.push_str(text);
                        let non_ws = text.chars().filter(|c| !c.is_whitespace()).count();
                        seg.total_chars += non_ws;
                        if run_bold {
                            seg.bold_chars += non_ws;
                        }
                        if let Some(sz) = run_size {
                            seg.max_font_pt =
                                Some(seg.max_font_pt.map_or(sz, |cur| cur.max(sz)));
                        }
                    }
                }
                "tab" => {
                    let seg = segments.last_mut().expect("at least one segment");
                    if !seg.text.ends_with("   ") {
                        seg.text.push_str("   ");
                    }
                }
                "br" | "cr" => {
                    segments.push(Segment::default());
                }
                _ => {}
            }
        }
    }

    walk_runs(p, &mut segments);

    for (idx, seg) in segments.into_iter().enumerate() {
        let normalized = normalize_line_text(&seg.text);
        if normalized.is_empty() {
            continue;
        }
        let (bullet_stripped, had_bullet_prefix) = strip_leading_bullet_marker(&normalized);
        let final_text = if had_bullet_prefix {
            bullet_stripped
        } else {
            normalized
        };
        if final_text.is_empty() {
            continue;
        }
        let is_bold = seg.total_chars > 0 && (seg.bold_chars * 4 >= seg.total_chars * 3);
        out.push(ExtractedLine::from_text(
            final_text,
            is_bold,
            is_heading && idx == 0,
            (is_list_item && idx == 0) || had_bullet_prefix,
            has_bottom_border,
            seg.max_font_pt.or(para_font_size),
            in_table,
        ));
    }
}

fn is_rpr_bold(rpr: Node<'_, '_>) -> bool {
    let Some(b) = rpr
        .children()
        .find(|n| n.is_element() && (n.tag_name().name() == "b" || n.tag_name().name() == "bCs"))
    else {
        return false;
    };
    match attr_by_local_name(b, "val") {
        Some(v) => !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "off"),
        None => true,
    }
}

fn rpr_font_size_pt(rpr: Node<'_, '_>) -> Option<f32> {
    let sz = rpr
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "sz")?;
    let val = attr_by_local_name(sz, "val")?;
    let half_pts = val.parse::<f32>().ok()?;
    Some(half_pts / 2.0)
}

fn attr_by_local_name<'a>(node: Node<'a, 'a>, local_name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == local_name)
        .map(|a| a.value())
}
