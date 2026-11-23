//! Native PDF export with standardized file naming.
//!
//! Child variations are named `{Company}_{Role}_CV.pdf`; roots (or children
//! without a complete job context) fall back to `{Title}.pdf`. Every filename
//! component is sanitized so user-controlled text can never escape the chosen
//! export directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::contracts::CvDocument;

fn is_windows_reserved_name(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Filesystem-safe, human-readable filename component.
pub fn sanitize_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_separator = false;

    for ch in input.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_separator = false;
        } else if !last_separator {
            out.push('_');
            last_separator = true;
        }
    }

    let trimmed = out.trim_matches('_');
    let mut cleaned: String = trimmed.chars().take(80).collect();
    while cleaned.ends_with('_') {
        cleaned.pop();
    }

    if cleaned.is_empty() {
        "VanguardCV".to_string()
    } else if is_windows_reserved_name(&cleaned) {
        format!("{cleaned}_CV")
    } else {
        cleaned
    }
}

/// Builds the export filename for a document.
pub fn export_filename(doc: &CvDocument) -> String {
    if let Some(job) = &doc.job_context
        && !job.company_name.trim().is_empty()
        && !job.target_role.trim().is_empty()
    {
        return format!(
            "{}_{}_CV.pdf",
            sanitize_component(&job.company_name),
            sanitize_component(&job.target_role)
        );
    }

    format!("{}.pdf", sanitize_component(&doc.title))
}

/// Default directory for exported PDFs.
pub fn default_export_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::download_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("VanguardCV")
}

/// Writes PDF bytes to `directory`, creating it if necessary.
///
/// A numeric suffix is added when a file with the same name already exists, so
/// exports never overwrite a previous application package.
pub fn write_pdf(doc: &CvDocument, pdf_bytes: &[u8], directory: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(directory)
        .with_context(|| format!("creating export directory {:?}", directory))?;

    let filename = export_filename(doc);
    let stem = Path::new(&filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("VanguardCV");

    let mut path = directory.join(&filename);
    let mut counter = 2;
    while path.exists() {
        path = directory.join(format!("{stem}_{counter}.pdf"));
        counter += 1;
    }

    std::fs::write(&path, pdf_bytes).with_context(|| format!("writing PDF to {:?}", path))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::JobContext;
    use crate::mocks::sample_root_document;

    #[test]
    fn sanitizes_path_traversal_and_punctuation() {
        assert_eq!(sanitize_component("../../etc/passwd"), "etc_passwd");
        assert_eq!(
            sanitize_component("CrowdStrike — Red Team"),
            "CrowdStrike_Red_Team"
        );
        assert_eq!(sanitize_component("   "), "VanguardCV");
        assert!(!sanitize_component("a/b\\c:d").contains('/'));
    }

    #[test]
    fn guards_windows_reserved_device_names() {
        assert_eq!(sanitize_component("CON"), "CON_CV");
        assert_eq!(sanitize_component("nul"), "nul_CV");
        assert_eq!(sanitize_component("Com1"), "Com1_CV");
        assert_eq!(sanitize_component("LPT9"), "LPT9_CV");
        assert_eq!(sanitize_component("Controller"), "Controller");
    }

    #[test]
    fn child_uses_company_and_role() {
        let mut doc = sample_root_document();
        doc.title = "CrowdStrike Red Team".to_string();
        doc.job_context = Some(JobContext {
            company_name: "CrowdStrike".to_string(),
            target_role: "Penetration Tester".to_string(),
            job_description_raw: String::new(),
            application_url: None,
            applied_skill_ids: Vec::new(),
            ats_match_score: None,
        });
        assert_eq!(
            export_filename(&doc),
            "CrowdStrike_Penetration_Tester_CV.pdf"
        );
    }

    #[test]
    fn root_falls_back_to_title() {
        let mut doc = sample_root_document();
        doc.title = "Cybersecurity & Pentesting Master".to_string();
        assert_eq!(export_filename(&doc), "Cybersecurity_Pentesting_Master.pdf");
    }

    #[test]
    fn writes_into_directory_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let doc = sample_root_document();
        let first = write_pdf(&doc, b"%PDF-1.7\n", dir.path()).unwrap();
        let second = write_pdf(&doc, b"%PDF-1.7\n", dir.path()).unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with(dir.path()));
        assert_eq!(std::fs::read(&first).unwrap(), b"%PDF-1.7\n");
    }
}
