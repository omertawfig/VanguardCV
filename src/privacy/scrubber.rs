//! `Aho-Corasick` dictionary + regex scrubber implementing
//! [`crate::contracts::PrivacyGuard`].
//!
//! # Design
//!
//! 1. **Dictionary layer:** An `AhoCorasick` automaton is built from every
//!    non-trivial string extracted from [`PiiContactHeader`] — full name,
//!    individual name tokens (≥ 3 chars), email + username prefix, raw phone
//!    digits and formatted variants, non-generic location, social URLs and
//!    handles, and custom field values. The automaton performs
//!    case-insensitive left-to-right scanning; the leftmost-longest match wins.
//!
//! 2. **Regex layer:** After the dictionary pass, residual stray emails, phone
//!    numbers, URLs, and IPv4/IPv6 addresses are matched by compiled `Regex`
//!    patterns (with date-range false-positive filtering on phone numbers).
//!
//! 3. **Token format:** `[REDACTED_PII_N]` where `N` is a global counter that
//!    increments across all replacements in a single scrub call. Tokens are
//!    unique within a payload, which lets the UI badge show a count and would
//!    support reversibility in future phases.
//!
//! 4. **JSON safety:** Text-field values are replaced **in memory** before
//!    serialisation; the JSON structure (keys, nesting) is never modified.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;

use crate::contracts::{
    PiiContactHeader, PiiKind, PiiMatch, PrivacyGuard, SanitizedCvBody, ScrubbedPayload,
    ScrubbedPayloadToken, SectionKind,
};

// ---------------------------------------------------------------------------
// Compiled static regex patterns (phone / email / url / ip)
// ---------------------------------------------------------------------------

static RE_EMAIL: OnceLock<Regex> = OnceLock::new();
static RE_PHONE: OnceLock<Regex> = OnceLock::new();
static RE_URL: OnceLock<Regex> = OnceLock::new();
static RE_IPV4: OnceLock<Regex> = OnceLock::new();
static RE_IPV6: OnceLock<Regex> = OnceLock::new();

fn re_email() -> &'static Regex {
    RE_EMAIL
        .get_or_init(|| Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap())
}

fn re_phone() -> &'static Regex {
    RE_PHONE.get_or_init(|| {
        // Matches:  +1 555 123 4567  |  (555) 123-4567  |  +44-20-7946-0958
        // Additional filtering in `is_probable_phone` requires >= 7 digits and
        // excludes date ranges such as "2020 - 2024".
        Regex::new(r"\+?[0-9][\s\-\(\).0-9]{6,}[0-9]").unwrap()
    })
}

fn re_url() -> &'static Regex {
    RE_URL.get_or_init(|| Regex::new(r#"https?://[^\s"'<>]+"#).unwrap())
}

fn re_ipv4() -> &'static Regex {
    RE_IPV4.get_or_init(|| {
        Regex::new(
            r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b",
        )
        .unwrap()
    })
}

fn re_ipv6() -> &'static Regex {
    RE_IPV6.get_or_init(|| {
        // Simplified: colon-separated hex groups (covers most practical cases)
        Regex::new(r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b").unwrap()
    })
}

/// Validates a `re_phone()` match to prevent false positives on year ranges
/// (`2020 - 2024`, `2019-2023`) or sparse digit sequences with < 7 digits.
fn is_probable_phone(matched: &str) -> bool {
    let digits = matched.chars().filter(|c| c.is_ascii_digit()).count();
    if digits < 7 {
        return false;
    }

    // Reject plain year ranges ("2020 - 2024", "2019-2023") and ISO dates ("2024-03-15").
    let trimmed = matched.trim();
    if !trimmed.contains('+')
        && !trimmed.contains('(')
        && !trimmed.contains(')')
        && !trimmed.contains('.')
        && digits == 8
    {
        let parts: Vec<&str> = trimmed.split('-').map(str::trim).collect();
        if parts.len() == 2
            && parts[0].len() == 4
            && parts[1].len() == 4
            && let (Ok(y1), Ok(y2)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>())
            && (1900..=2100).contains(&y1)
            && (1900..=2100).contains(&y2)
        {
            return false;
        }
        if parts.len() == 3
            && parts[0].len() == 4
            && parts[1].len() == 2
            && parts[2].len() == 2
            && let (Ok(y), Ok(m), Ok(d)) = (
                parts[0].parse::<u32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            )
            && (1900..=2100).contains(&y)
            && (1..=12).contains(&m)
            && (1..=31).contains(&d)
        {
            return false;
        }
    }

    true
}

/// Prevents single-word alphanumeric dictionary tokens (e.g. "Jan", "Lee", "dev")
/// from corrupting unrelated words ("January", "developer") while keeping
/// punctuation-bounded and multi-token matches intact.
fn respects_word_boundaries(input: &str, start: usize, end: usize) -> bool {
    let matched = &input[start..end];
    if !matched.chars().all(|c| c.is_alphanumeric()) {
        return true;
    }
    let prev_alnum = input[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric());
    let next_alnum = input[end..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric());
    !prev_alnum && !next_alnum
}

fn is_generic_work_mode(location: &str) -> bool {
    matches!(
        location.trim().to_ascii_lowercase().as_str(),
        "remote" | "hybrid" | "onsite" | "on-site" | "worldwide" | "global"
    )
}

// ---------------------------------------------------------------------------
// Dictionary builder
// ---------------------------------------------------------------------------

/// Collects every string from the PII header that should be redacted.
///
/// Returns `(patterns, kinds)` parallel vectors — `patterns[i]` is the string
/// literal to match and `kinds[i]` is the [`PiiKind`] label for it.
fn build_dictionary(pii: &PiiContactHeader) -> (Vec<String>, Vec<PiiKind>) {
    let mut patterns: Vec<String> = Vec::new();
    let mut kinds: Vec<PiiKind> = Vec::new();

    let mut push = |s: &str, kind: PiiKind| {
        let s = s.trim();
        if s.len() >= 3 {
            patterns.push(s.to_string());
            kinds.push(kind);
        }
    };

    // Full name
    push(&pii.full_name, PiiKind::FullName);

    // Individual name tokens >= 3 chars
    for token in pii.full_name.split_whitespace() {
        if token.len() >= 3 {
            push(token, PiiKind::NameToken);
        }
    }

    // Email + username prefix
    if let Some(email) = pii.email.as_deref().filter(|e| !e.trim().is_empty()) {
        push(email, PiiKind::Email);
        // Username prefix (before @)
        if let Some(prefix) = email.split('@').next()
            && prefix.len() >= 3
        {
            push(prefix, PiiKind::Email);
        }
    }

    // Phone — raw digits and formatted variants
    if let Some(phone) = pii.phone.as_deref().filter(|p| !p.trim().is_empty()) {
        push(phone, PiiKind::Phone);
        // Raw digits only variant
        let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 7 {
            push(&digits, PiiKind::Phone);
        }
    }

    // Non-generic location (skip generic work-mode terms like "Remote")
    if let Some(location) = pii.location.as_deref().filter(|l| !l.trim().is_empty())
        && !is_generic_work_mode(location)
    {
        push(location, PiiKind::CustomField);
    }

    // Social URLs and handles (scrub even if disabled in header)
    for link in &pii.social_links {
        push(&link.url, PiiKind::SocialUrl);
        push(&link.display_text, PiiKind::SocialHandle);
        let trimmed_url = link.url.trim().trim_end_matches('/');
        if let Some(slug) = trimmed_url.rsplit('/').next() {
            push(slug, PiiKind::SocialHandle);
        }
    }

    // Custom fields (scrub even if disabled in header)
    for field in &pii.custom_fields {
        if !field.value.trim().is_empty() {
            push(&field.value, PiiKind::CustomField);
        }
    }

    (patterns, kinds)
}

// ---------------------------------------------------------------------------
// Core scrubbing engine
// ---------------------------------------------------------------------------

/// Scrubs `text` using the given dictionary automaton and then regex layers.
///
/// `counter` is updated in place; each replacement increments it.
fn scrub_text(text: &str, ac: Option<&AhoCorasick>, counter: &mut usize) -> String {
    let mut out = text.to_string();

    // --- Aho-Corasick dictionary pass ---
    if let Some(ac) = ac {
        let input = out.clone();
        let mut result = String::with_capacity(input.len());
        let mut last_end = 0usize;
        for mat in ac.find_iter(&input) {
            if !respects_word_boundaries(&input, mat.start(), mat.end()) {
                continue;
            }
            result.push_str(&input[last_end..mat.start()]);
            *counter += 1;
            result.push_str(&format!("[REDACTED_PII_{}]", counter));
            last_end = mat.end();
        }
        result.push_str(&input[last_end..]);
        out = result;
    }

    // --- Regex passes ---
    let regex_layers: &[(&Regex, bool)] = &[
        (re_email(), false),
        (re_url(), false),
        (re_ipv4(), false),
        (re_ipv6(), false),
        (re_phone(), true),
    ];

    for (re, is_phone) in regex_layers {
        let input = out.clone();
        let mut result = String::with_capacity(input.len());
        let mut last_end = 0usize;
        for mat in re.find_iter(&input) {
            if *is_phone && !is_probable_phone(mat.as_str()) {
                continue;
            }
            result.push_str(&input[last_end..mat.start()]);
            *counter += 1;
            result.push_str(&format!("[REDACTED_PII_{}]", counter));
            last_end = mat.end();
        }
        result.push_str(&input[last_end..]);
        out = result;
    }

    out
}

/// Builds an `AhoCorasick` automaton from the given patterns.
/// Returns `None` if there are no patterns (empty PII header).
fn build_ac(patterns: &[String]) -> Option<AhoCorasick> {
    if patterns.is_empty() {
        return None;
    }
    Some(
        AhoCorasickBuilder::new()
            .ascii_case_insensitive(true)
            .match_kind(MatchKind::LeftmostLongest)
            .build(patterns)
            .expect("aho-corasick build from PII dictionary"),
    )
}

/// Scrubs all text fields in `body`, returning the sanitized body and the
/// total number of replacements made.
fn scrub_body(body: &SanitizedCvBody, ac: Option<&AhoCorasick>) -> (SanitizedCvBody, usize) {
    let mut counter = 0usize;
    let mut scrubbed = body.clone();

    for section in scrubbed.sections.iter_mut() {
        section.title = scrub_text(&section.title, ac, &mut counter);

        match &mut section.kind {
            SectionKind::Summary { text } => {
                *text = scrub_text(text, ac, &mut counter);
            }
            SectionKind::TimelineEntries { entries } => {
                for entry in entries.iter_mut() {
                    entry.primary_heading = scrub_text(&entry.primary_heading, ac, &mut counter);
                    if let Some(h) = &mut entry.secondary_heading {
                        *h = scrub_text(h, ac, &mut counter);
                    }
                    for tag in entry.tags.iter_mut() {
                        *tag = scrub_text(tag, ac, &mut counter);
                    }
                    for bullet in entry.bullets.iter_mut() {
                        bullet.text = scrub_text(&bullet.text, ac, &mut counter);
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for cat in categories.iter_mut() {
                    cat.category_label = scrub_text(&cat.category_label, ac, &mut counter);
                    for item in cat.items.iter_mut() {
                        *item = scrub_text(item, ac, &mut counter);
                    }
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for item in items.iter_mut() {
                    item.text = scrub_text(&item.text, ac, &mut counter);
                }
            }
        }
    }

    (scrubbed, counter)
}

// ---------------------------------------------------------------------------
// PiiScrubber — public implementation
// ---------------------------------------------------------------------------

/// Production implementation of [`PrivacyGuard`].
#[derive(Default)]
pub struct PiiScrubber;

impl PrivacyGuard for PiiScrubber {
    fn scrub(&self, body: &SanitizedCvBody, pii: &PiiContactHeader) -> Result<ScrubbedPayload> {
        let (patterns, _kinds) = build_dictionary(pii);
        let ac = build_ac(&patterns);
        let (scrubbed_body, redaction_count) = scrub_body(body, ac.as_ref());
        let json = serde_json::to_string(&scrubbed_body)?;
        let token = ScrubbedPayloadToken::mint(&json);
        Ok(ScrubbedPayload {
            body: scrubbed_body,
            json,
            token,
            redaction_count,
        })
    }

    fn inspect_outbound_payload(
        &self,
        body: &SanitizedCvBody,
        pii: &PiiContactHeader,
    ) -> Result<String> {
        let payload = self.scrub(body, pii)?;
        Ok(payload.json)
    }

    fn scan(&self, text: &str, pii: &PiiContactHeader) -> Vec<PiiMatch> {
        let (patterns, kinds) = build_dictionary(pii);
        let mut out: Vec<PiiMatch> = Vec::new();

        // --- Dictionary scan ---
        if let Some(ac) = build_ac(&patterns) {
            for mat in ac.find_iter(text) {
                if !respects_word_boundaries(text, mat.start(), mat.end()) {
                    continue;
                }
                let kind = kinds
                    .get(mat.pattern().as_usize())
                    .copied()
                    .unwrap_or(PiiKind::CustomField);
                out.push(PiiMatch {
                    start: mat.start(),
                    end: mat.end(),
                    token: String::new(),
                    kind,
                });
            }
        }

        // --- Regex scan ---
        let regex_layers: &[(&Regex, PiiKind, bool)] = &[
            (re_email(), PiiKind::RegexEmail, false),
            (re_url(), PiiKind::RegexUrl, false),
            (re_ipv4(), PiiKind::IpAddress, false),
            (re_ipv6(), PiiKind::IpAddress, false),
            (re_phone(), PiiKind::RegexPhone, true),
        ];

        for (re, kind, is_phone) in regex_layers {
            for mat in re.find_iter(text) {
                if *is_phone && !is_probable_phone(mat.as_str()) {
                    continue;
                }
                // Skip if already covered by an existing match
                let already_covered = out
                    .iter()
                    .any(|m| m.start <= mat.start() && mat.end() <= m.end);
                if already_covered {
                    continue;
                }
                out.push(PiiMatch {
                    start: mat.start(),
                    end: mat.end(),
                    token: String::new(),
                    kind: *kind,
                });
            }
        }

        // Sort by start offset and assign deterministic sequential token labels
        out.sort_by_key(|m| (m.start, m.end));
        for (idx, m) in out.iter_mut().enumerate() {
            m.token = format!("[REDACTED_PII_{}]", idx + 1);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        BulletItem, CustomHeaderField, CvSection, PrivacyGuard, SanitizedCvBody, SectionKind,
        SocialLink,
    };
    use uuid::Uuid;

    fn sample_pii() -> PiiContactHeader {
        PiiContactHeader {
            full_name: "Jordan Blake".to_string(),
            headline_job_title: Some("Offensive Security Engineer".to_string()),
            email: Some("jordan.blake@protonmail.com".to_string()),
            phone: Some("+1 (555) 847-2991".to_string()),
            location: Some("Remote".to_string()),
            social_links: vec![
                SocialLink {
                    id: Uuid::new_v4(),
                    platform: "LinkedIn".to_string(),
                    display_text: "linkedin.com/in/jordanblake".to_string(),
                    url: "https://linkedin.com/in/jordanblake".to_string(),
                    enabled: true,
                },
                SocialLink {
                    id: Uuid::new_v4(),
                    platform: "GitHub".to_string(),
                    display_text: "github.com/jordanblake".to_string(),
                    url: "https://github.com/jordanblake".to_string(),
                    enabled: true,
                },
            ],
            custom_fields: vec![CustomHeaderField {
                id: Uuid::new_v4(),
                label: "Portfolio".to_string(),
                value: "jordanblake.dev".to_string(),
                enabled: true,
            }],
        }
    }

    fn body_with_text(text: &str) -> SanitizedCvBody {
        SanitizedCvBody {
            sections: vec![CvSection {
                id: Uuid::new_v4(),
                origin_item_id: None,
                title: "EXPERIENCE".to_string(),
                enabled: true,
                kind: SectionKind::SimpleBulletList {
                    items: vec![BulletItem {
                        id: Uuid::new_v4(),
                        origin_item_id: None,
                        enabled: true,
                        text: text.to_string(),
                    }],
                },
            }],
        }
    }

    fn extract_bullet_text(body: &SanitizedCvBody) -> String {
        match &body.sections[0].kind {
            SectionKind::SimpleBulletList { items } => items[0].text.clone(),
            _ => panic!("expected simple bullet list"),
        }
    }

    // --- Name is redacted ---

    #[test]
    fn full_name_is_redacted() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let body = body_with_text("Jordan Blake did amazing work on this project.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            !text.contains("Jordan"),
            "name 'Jordan' should be redacted; got: {text}"
        );
        assert!(
            !text.contains("Blake"),
            "name 'Blake' should be redacted; got: {text}"
        );
        assert!(
            text.contains("[REDACTED_PII_"),
            "expected REDACTED token; got: {text}"
        );
    }

    // --- Email is redacted in full (no @domain leak) ---

    #[test]
    fn email_is_redacted() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let body = body_with_text("Contact me at jordan.blake@protonmail.com for details.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            !text.contains("jordan.blake@protonmail.com"),
            "email should be redacted; got: {text}"
        );
        assert!(
            !text.contains("@protonmail.com"),
            "email domain must NOT leak when name token matches prefix; got: {text}"
        );
        assert!(payload.redaction_count > 0);
    }

    // --- Date ranges are not falsely redacted as phone numbers ---

    #[test]
    fn date_range_is_not_redacted_as_phone() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let body =
            body_with_text("Led security engineering 2020 - 2024 and SOC automation 2019-2023.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            text.contains("2020 - 2024"),
            "date range '2020 - 2024' should not be redacted; got: {text}"
        );
        assert!(
            text.contains("2019-2023"),
            "date range '2019-2023' should not be redacted; got: {text}"
        );
        assert_eq!(payload.redaction_count, 0);
    }

    // --- Disabled social/custom fields are still redacted ---

    #[test]
    fn disabled_social_and_custom_fields_are_still_redacted() {
        let scrubber = PiiScrubber;
        let mut pii = sample_pii();
        pii.social_links[0].enabled = false;
        pii.social_links[0].url = "https://linkedin.com/in/secret-handle/".to_string();
        pii.custom_fields[0].enabled = false;
        pii.location = Some("London, UK".to_string());

        let body = body_with_text(
            "Based in London, UK; portfolio jordanblake.dev and profile secret-handle.",
        );

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(!text.contains("London, UK"), "location leaked: {text}");
        assert!(
            !text.contains("jordanblake.dev"),
            "disabled custom field leaked: {text}"
        );
        assert!(
            !text.contains("secret-handle"),
            "disabled social handle leaked: {text}"
        );
    }

    // --- Phone is redacted ---

    #[test]
    fn phone_is_redacted() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let body = body_with_text("Call me at +1 (555) 847-2991 anytime.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            !text.contains("847-2991"),
            "phone should be redacted; got: {text}"
        );
    }

    // --- Social handle / URL is redacted ---

    #[test]
    fn social_handle_is_redacted() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let body = body_with_text(
            "See my work at https://github.com/jordanblake or linkedin.com/in/jordanblake.",
        );

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            !text.contains("jordanblake"),
            "social handle should be redacted; got: {text}"
        );
        assert!(
            !text.contains("github.com/jordanblake"),
            "social URL should be redacted; got: {text}"
        );
    }

    // --- Non-PII text is unchanged ---

    #[test]
    fn non_pii_text_unchanged() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let body = body_with_text(
            "Developed a Rust-based security tool reducing incident response time by 30%.",
        );

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            text.contains("Rust-based security tool"),
            "non-PII text should be preserved; got: {text}"
        );
        assert!(
            text.contains("30%"),
            "percentage should be preserved; got: {text}"
        );
        assert_eq!(payload.redaction_count, 0);
    }

    // --- Inspect outbound payload is clean JSON ---

    #[test]
    fn inspect_outbound_payload_contains_no_pii() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let body = body_with_text(
            "Jordan Blake found a critical bug affecting jordan.blake@protonmail.com users.",
        );

        let json = scrubber.inspect_outbound_payload(&body, &pii).unwrap();

        assert!(!json.contains("Jordan"), "name in payload: {json}");
        assert!(!json.contains("Blake"), "name in payload: {json}");
        assert!(
            !json.contains("jordan.blake@protonmail.com"),
            "email in payload: {json}"
        );
        assert!(
            !json.contains("@protonmail.com"),
            "email domain in payload: {json}"
        );
        // JSON structure should still be valid
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed.is_object());
    }

    // --- scan() returns PiiMatch offsets ---

    #[test]
    fn scan_returns_matches_with_offsets() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        let text = "Jordan Blake reported a bug via jordan.blake@protonmail.com.";

        let matches = scrubber.scan(text, &pii);
        assert!(!matches.is_empty(), "expected matches; got none");

        // Verify offset correctness
        for m in &matches {
            assert!(m.start < m.end);
            assert!(m.end <= text.len());
            let matched_str = &text[m.start..m.end];
            assert!(!matched_str.is_empty());
        }
    }

    // --- Stray regex email (not in PII header) is redacted ---

    #[test]
    fn stray_email_is_redacted_by_regex() {
        let scrubber = PiiScrubber;
        // PII header has no email
        let pii = PiiContactHeader {
            full_name: "Alex Smith".to_string(),
            ..Default::default()
        };
        let body = body_with_text("Contact the team at attacker@evil.com for the report.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            !text.contains("attacker@evil.com"),
            "stray email should be redacted; got: {text}"
        );
    }

    // --- IPv4 address is redacted ---

    #[test]
    fn ipv4_is_redacted() {
        let scrubber = PiiScrubber;
        let pii = PiiContactHeader::default();
        let body = body_with_text("Server at 192.168.1.42 was targeted.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            !text.contains("192.168.1.42"),
            "IPv4 should be redacted; got: {text}"
        );
    }

    // --- Token count is accurate ---

    #[test]
    fn redaction_count_is_accurate() {
        let scrubber = PiiScrubber;
        let pii = sample_pii();
        // Contains both name and email -> at least 2 redactions
        let body = body_with_text("Jordan Blake contacted us at jordan.blake@protonmail.com.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        assert!(
            payload.redaction_count >= 2,
            "expected at least 2 redactions; got {}",
            payload.redaction_count
        );
    }

    // --- Case-insensitive matching ---

    #[test]
    fn case_insensitive_match() {
        let scrubber = PiiScrubber;
        let pii = PiiContactHeader {
            full_name: "Jordan Blake".to_string(),
            ..Default::default()
        };
        let body = body_with_text("JORDAN BLAKE performed the security review.");

        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);

        assert!(
            !text.contains("JORDAN"),
            "case-insensitive redaction failed; got: {text}"
        );
    }

    // --- Token minting via ScrubbedPayloadToken::mint ---

    #[test]
    fn token_is_minted_correctly() {
        let scrubber = PiiScrubber;
        let pii = PiiContactHeader::default();
        let body = SanitizedCvBody::default();

        let payload = scrubber.scrub(&body, &pii).unwrap();
        // Token nonce must be a valid UUID
        let nonce = payload.token.nonce();
        assert_ne!(nonce, uuid::Uuid::nil());
    }

    #[test]
    fn iso_date_is_not_redacted_as_phone_and_ipv4_classified_as_ip() {
        let scrubber = PiiScrubber;
        let pii = PiiContactHeader::default();
        let body = body_with_text("Released audit patch on 2024-03-15 for 192.168.1.42.");
        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);
        assert!(text.contains("2024-03-15"), "ISO date redacted: {text}");
        assert!(!text.contains("192.168.1.42"), "IPv4 leaked: {text}");

        let matches = scrubber.scan("Host 192.168.1.42 on 2024-03-15", &pii);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].kind, PiiKind::IpAddress);
    }

    #[test]
    fn short_name_token_respects_word_boundaries() {
        let scrubber = PiiScrubber;
        let pii = PiiContactHeader {
            full_name: "Jan Lee".to_string(),
            ..Default::default()
        };
        let body = body_with_text("Built Django service in January 2024; reviewed by Jan.");
        let payload = scrubber.scrub(&body, &pii).unwrap();
        let text = extract_bullet_text(&payload.body);
        assert!(text.contains("Django"), "Django corrupted: {text}");
        assert!(text.contains("January"), "January corrupted: {text}");
        assert!(!text.contains("by Jan."), "standalone Jan leaked: {text}");
    }
}
