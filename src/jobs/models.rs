//! Domain models for the Job Discovery tab (closed-loop CV-to-Job workflow).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::contracts::DocumentId;

/// Default number of job postings requested per page.
pub const DEFAULT_PAGE_SIZE: usize = 10;
/// Hard ceiling on job postings per page to avoid hammering upstream APIs or the user's UI.
pub const MAX_PAGE_SIZE: usize = 20;
/// Hard ceiling on navigable page depth (`MAX_PAGE_NUMBER * MAX_PAGE_SIZE` = 200 max offset).
pub const MAX_PAGE_NUMBER: usize = 10;

/// Supported country option for Job Discovery filtering (Freehire API `countries`,
/// LinkedIn `location`, and regional Indeed host + `l`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountryOption {
    /// Lowercase ISO 3166-1 alpha-2 code (used by Freehire `countries=<code>`).
    pub code: &'static str,
    /// Human-readable country name (used in the UI dropdown and LinkedIn/Indeed location fields).
    pub name: &'static str,
    /// Regional Indeed hostname (`"www.indeed.com"` for US / fallback countries).
    pub indeed_host: &'static str,
}

/// Curated alphabetical catalog of hiring countries supported across Freehire,
/// LinkedIn, and Indeed.
pub const JOB_COUNTRIES: &[CountryOption] = &[
    CountryOption {
        code: "ar",
        name: "Argentina",
        indeed_host: "ar.indeed.com",
    },
    CountryOption {
        code: "au",
        name: "Australia",
        indeed_host: "au.indeed.com",
    },
    CountryOption {
        code: "at",
        name: "Austria",
        indeed_host: "at.indeed.com",
    },
    CountryOption {
        code: "bh",
        name: "Bahrain",
        indeed_host: "bh.indeed.com",
    },
    CountryOption {
        code: "be",
        name: "Belgium",
        indeed_host: "be.indeed.com",
    },
    CountryOption {
        code: "br",
        name: "Brazil",
        indeed_host: "br.indeed.com",
    },
    CountryOption {
        code: "ca",
        name: "Canada",
        indeed_host: "ca.indeed.com",
    },
    CountryOption {
        code: "cz",
        name: "Czechia",
        indeed_host: "cz.indeed.com",
    },
    CountryOption {
        code: "dk",
        name: "Denmark",
        indeed_host: "dk.indeed.com",
    },
    CountryOption {
        code: "eg",
        name: "Egypt",
        indeed_host: "eg.indeed.com",
    },
    CountryOption {
        code: "fi",
        name: "Finland",
        indeed_host: "fi.indeed.com",
    },
    CountryOption {
        code: "fr",
        name: "France",
        indeed_host: "fr.indeed.com",
    },
    CountryOption {
        code: "de",
        name: "Germany",
        indeed_host: "de.indeed.com",
    },
    CountryOption {
        code: "gr",
        name: "Greece",
        indeed_host: "gr.indeed.com",
    },
    CountryOption {
        code: "hk",
        name: "Hong Kong",
        indeed_host: "hk.indeed.com",
    },
    CountryOption {
        code: "in",
        name: "India",
        indeed_host: "in.indeed.com",
    },
    CountryOption {
        code: "id",
        name: "Indonesia",
        indeed_host: "id.indeed.com",
    },
    CountryOption {
        code: "ie",
        name: "Ireland",
        indeed_host: "ie.indeed.com",
    },
    CountryOption {
        code: "il",
        name: "Israel",
        indeed_host: "il.indeed.com",
    },
    CountryOption {
        code: "it",
        name: "Italy",
        indeed_host: "it.indeed.com",
    },
    CountryOption {
        code: "jp",
        name: "Japan",
        indeed_host: "jp.indeed.com",
    },
    CountryOption {
        code: "jo",
        name: "Jordan",
        indeed_host: "www.indeed.com",
    },
    CountryOption {
        code: "kw",
        name: "Kuwait",
        indeed_host: "kw.indeed.com",
    },
    CountryOption {
        code: "my",
        name: "Malaysia",
        indeed_host: "my.indeed.com",
    },
    CountryOption {
        code: "mx",
        name: "Mexico",
        indeed_host: "mx.indeed.com",
    },
    CountryOption {
        code: "ma",
        name: "Morocco",
        indeed_host: "ma.indeed.com",
    },
    CountryOption {
        code: "nl",
        name: "Netherlands",
        indeed_host: "nl.indeed.com",
    },
    CountryOption {
        code: "nz",
        name: "New Zealand",
        indeed_host: "nz.indeed.com",
    },
    CountryOption {
        code: "no",
        name: "Norway",
        indeed_host: "no.indeed.com",
    },
    CountryOption {
        code: "om",
        name: "Oman",
        indeed_host: "om.indeed.com",
    },
    CountryOption {
        code: "pl",
        name: "Poland",
        indeed_host: "pl.indeed.com",
    },
    CountryOption {
        code: "pt",
        name: "Portugal",
        indeed_host: "pt.indeed.com",
    },
    CountryOption {
        code: "qa",
        name: "Qatar",
        indeed_host: "qa.indeed.com",
    },
    CountryOption {
        code: "ro",
        name: "Romania",
        indeed_host: "ro.indeed.com",
    },
    CountryOption {
        code: "sa",
        name: "Saudi Arabia",
        indeed_host: "sa.indeed.com",
    },
    CountryOption {
        code: "sg",
        name: "Singapore",
        indeed_host: "sg.indeed.com",
    },
    CountryOption {
        code: "za",
        name: "South Africa",
        indeed_host: "za.indeed.com",
    },
    CountryOption {
        code: "kr",
        name: "South Korea",
        indeed_host: "kr.indeed.com",
    },
    CountryOption {
        code: "es",
        name: "Spain",
        indeed_host: "es.indeed.com",
    },
    CountryOption {
        code: "se",
        name: "Sweden",
        indeed_host: "se.indeed.com",
    },
    CountryOption {
        code: "ch",
        name: "Switzerland",
        indeed_host: "ch.indeed.com",
    },
    CountryOption {
        code: "tr",
        name: "Turkey",
        indeed_host: "tr.indeed.com",
    },
    CountryOption {
        code: "ae",
        name: "United Arab Emirates",
        indeed_host: "ae.indeed.com",
    },
    CountryOption {
        code: "gb",
        name: "United Kingdom",
        indeed_host: "uk.indeed.com",
    },
    CountryOption {
        code: "us",
        name: "United States",
        indeed_host: "www.indeed.com",
    },
];

/// Looks up a country metadata entry by ISO 3166-1 alpha-2 code (case-insensitive).
pub fn find_country_by_code(code: &str) -> Option<&'static CountryOption> {
    let clean = code.trim();
    if clean.is_empty() {
        return None;
    }
    JOB_COUNTRIES
        .iter()
        .find(|c| c.code.eq_ignore_ascii_case(clean))
}

/// Validates and normalizes an ISO 3166-1 alpha-2 country code against [`JOB_COUNTRIES`].
pub fn normalize_country_code(raw: &str) -> Option<String> {
    find_country_by_code(raw).map(|c| c.code.to_string())
}

/// Which saved CV(s) the Job Discovery tab bases its search profiles on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CvSearchSource {
    /// Analyze a single saved CV (Master or Variation).
    Single(DocumentId),
    /// Synthesize broader search profiles across all saved CVs.
    AllCvs,
}

impl CvSearchSource {
    pub fn cache_key_prefix(&self) -> String {
        match self {
            CvSearchSource::Single(id) => format!("cv:{id}"),
            CvSearchSource::AllCvs => "all_cvs".to_string(),
        }
    }
}

/// Persisted per-CV search profile record stored in SQLite `settings` so
/// restarting the app reuses extracted keywords with 0 AI calls until the CV changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedProfileEntry {
    pub content_hash: String,
    pub profiles: Vec<SearchProfile>,
    #[serde(default)]
    pub used_fallback: bool,
}

/// A concise, realistic search profile generated from a scrubbed CV (or tweaked
/// manually by the user).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchProfile {
    pub id: String,
    /// Category label such as `"Primary Role"`, `"Adjacent Role"`, or `"Niche / Stack"`.
    pub label: String,
    /// Short, realistic target job title (e.g. `"Senior Security Engineer"`).
    pub title: String,
    /// 2–4 high-signal keywords/skills associated with this role.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Whether this profile is currently included in live job searches.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Individual keywords toggled off by the user without re-running AI.
    #[serde(default)]
    pub disabled_keywords: HashSet<String>,
}

fn default_true() -> bool {
    true
}

fn sanitize_query_term(term: &str) -> String {
    term.replace(['"', '\n', '\r', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl SearchProfile {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        title: impl Into<String>,
        keywords: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            title: title.into(),
            keywords,
            enabled: true,
            disabled_keywords: HashSet::new(),
        }
    }

    /// Returns currently enabled keywords for this profile.
    pub fn active_keywords(&self) -> Vec<&str> {
        self.keywords
            .iter()
            .filter(|kw| !self.disabled_keywords.contains(kw.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// Returns enabled keywords not already verbatim inside the role title (capped at `max`).
    pub fn distinct_active_keywords(&self, max: usize) -> Vec<String> {
        let title_lower = self.title.trim().to_ascii_lowercase();
        let mut out = Vec::new();
        for kw in self.active_keywords() {
            let clean = sanitize_query_term(kw);
            if clean.is_empty() {
                continue;
            }
            if title_lower.contains(&clean.to_ascii_lowercase()) {
                continue;
            }
            out.push(clean);
            if out.len() >= max {
                break;
            }
        }
        out
    }

    /// Builds a Freehire OpenAPI 3.0.3 `title:` clause for this profile
    /// (e.g., `title:"Security Researcher"` or `title:Pentester`).
    ///
    /// Using `title:` keeps Freehire matches focused on real role titles rather than
    /// noisy description mentions and lets up to 5 profiles combine with `OR` in one request.
    pub fn to_freehire_title_clause(&self) -> Option<String> {
        let title_clean = sanitize_query_term(&self.title);
        if title_clean.is_empty() {
            return None;
        }
        if title_clean.contains(' ') {
            Some(format!("title:\"{title_clean}\""))
        } else {
            Some(format!("title:{title_clean}"))
        }
    }

    /// Builds a concise general search string (title + up to 2 keywords).
    pub fn to_search_query(&self) -> String {
        let title_clean = sanitize_query_term(&self.title);
        let mut parts: Vec<String> = Vec::new();
        if !title_clean.is_empty() {
            parts.push(title_clean);
        }
        parts.extend(self.distinct_active_keywords(2));
        parts.join(" ")
    }

    /// Builds a broader fallback query using only the role title.
    pub fn broad_title_query(&self) -> String {
        sanitize_query_term(&self.title)
    }

    /// Builds a LinkedIn-optimized Boolean search query:
    /// `"Role Title" AND (Skill1 OR Skill2)`.
    pub fn to_linkedin_query(&self) -> String {
        let title = sanitize_query_term(&self.title);
        let kws = self.distinct_active_keywords(2);
        let kw_expr = if kws.is_empty() {
            String::new()
        } else if kws.len() == 1 {
            format_boolean_term(&kws[0])
        } else {
            let joined = kws
                .iter()
                .map(|k| format_boolean_term(k))
                .collect::<Vec<_>>()
                .join(" OR ");
            format!("({joined})")
        };

        match (title.is_empty(), kw_expr.is_empty()) {
            (false, false) => format!("{} AND {kw_expr}", format_boolean_term(&title)),
            (false, true) => format_boolean_term(&title),
            (true, false) => kw_expr,
            (true, true) => String::new(),
        }
    }

    /// Builds an Indeed-optimized Boolean search query with explicit `AND`:
    /// `"Role Title" AND (Skill1 OR Skill2)`.
    pub fn to_indeed_query(&self) -> String {
        let title = sanitize_query_term(&self.title);
        let kws = self.distinct_active_keywords(2);
        let kw_expr = if kws.is_empty() {
            String::new()
        } else if kws.len() == 1 {
            format_boolean_term(&kws[0])
        } else {
            let joined = kws
                .iter()
                .map(|k| format_boolean_term(k))
                .collect::<Vec<_>>()
                .join(" OR ");
            format!("({joined})")
        };

        match (title.is_empty(), kw_expr.is_empty()) {
            (false, false) => format!("{} AND {kw_expr}", format_boolean_term(&title)),
            (false, true) => format_boolean_term(&title),
            (true, false) => kw_expr,
            (true, true) => String::new(),
        }
    }
}

pub(crate) fn format_boolean_term(term: &str) -> String {
    let clean = sanitize_query_term(term);
    if clean.contains(' ') {
        format!("\"{clean}\"")
    } else {
        clean
    }
}

/// User-controlled search filters and pagination state that can be adjusted
/// without re-running AI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFilters {
    pub remote_only: bool,
    /// Optional lowercase ISO 3166-1 alpha-2 country code (`None` = All locations / Worldwide).
    pub country: Option<String>,
    /// Freshness window in days (`None` = any time, `Some(1 | 7 | 14 | 30)`).
    /// Defaults to `Some(1)` (last 24 hours).
    pub posted_within_days: Option<u32>,
    /// Optional manual query override entered by the user.
    pub manual_query: String,
    /// 1-indexed page number (`1..=MAX_PAGE_NUMBER`).
    pub page: usize,
    /// Number of postings to fetch per page (`5..=MAX_PAGE_SIZE`).
    pub page_size: usize,
}

impl Default for JobFilters {
    fn default() -> Self {
        Self {
            remote_only: false,
            country: None,
            posted_within_days: Some(1),
            manual_query: String::new(),
            page: 1,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}

impl JobFilters {
    pub fn clamped_page(&self) -> usize {
        self.page.clamp(1, MAX_PAGE_NUMBER)
    }

    pub fn clamped_page_size(&self) -> usize {
        self.page_size.clamp(5, MAX_PAGE_SIZE)
    }

    pub fn offset(&self) -> usize {
        (self.clamped_page() - 1) * self.clamped_page_size()
    }

    pub fn selected_country(&self) -> Option<&'static CountryOption> {
        self.country.as_deref().and_then(find_country_by_code)
    }

    pub fn cache_signature(&self, profiles: &[SearchProfile]) -> String {
        let manual = self.manual_query.trim().to_ascii_lowercase();
        let active_queries: Vec<String> = if !manual.is_empty() {
            vec![format!("manual:{manual}")]
        } else {
            profiles
                .iter()
                .filter(|p| p.enabled)
                .map(|p| {
                    let kws = p.distinct_active_keywords(2).join(",").to_ascii_lowercase();
                    format!("{}[{kws}]", p.title.trim().to_ascii_lowercase())
                })
                .filter(|q| !q.is_empty())
                .collect()
        };
        format!(
            "remote={};country={};days={};page={};size={};q={}",
            self.remote_only,
            self.country
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| "all".to_string()),
            self.posted_within_days
                .map(|d| d.to_string())
                .unwrap_or_else(|| "any".to_string()),
            self.clamped_page(),
            self.clamped_page_size(),
            active_queries.join("|")
        )
    }
}

/// Normalized, UI-ready job posting returned by a [`crate::jobs::provider::JobProvider`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobListing {
    pub public_slug: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub work_mode: Option<String>,
    pub salary_display: Option<String>,
    pub seniority: Option<String>,
    pub employment_type: Option<String>,
    pub skills: Vec<String>,
    /// Concise plain-text summary shown on the discovery card.
    pub preview: String,
    /// Full Markdown job description passed directly to CV tailoring on 1 click.
    pub description_markdown: String,
    /// Employer's direct application URL (falls back to posting page when `direct_apply`).
    pub apply_url: String,
    /// Canonical listing URL (`https://freehire.me/jobs/{public_slug}`).
    pub freehire_url: String,
    pub posted_at: Option<String>,
    /// Verified age in whole days (from `reality.age_days` or `created_at` / `posted_at`).
    pub age_days: Option<i64>,
    /// Unix timestamp (seconds) used for deterministic newest-first sorting within a day.
    pub sort_timestamp: Option<i64>,
    pub age_label: Option<String>,
    pub source: Option<String>,
    /// Name of the search profile that surfaced this opportunity.
    pub matched_profile: Option<String>,
}

/// Outcome of a paginated live job search.
#[derive(Debug, Clone, Default)]
pub struct JobSearchResult {
    pub jobs: Vec<JobListing>,
    pub total_estimated: u64,
    pub page: usize,
    pub page_size: usize,
    pub has_next_page: bool,
    pub from_cache: bool,
    pub warnings: Vec<String>,
}

/// Optional deep link to search an external job board with an optimized query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalSearchLink {
    pub platform: &'static str,
    pub url: String,
}
