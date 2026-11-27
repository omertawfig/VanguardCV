//! Job discovery provider trait, Freehire.me public API integration, and
//! external board deep-link builders (LinkedIn & Indeed).
//!
//! Verified against Freehire's OpenAPI 3.0.3 specification:
//! - Primary endpoint: `GET https://freehire.me/api/v1/agent/jobs/search`
//!   with `description_format=markdown`, `work_mode=remote`, `countries=<code>`,
//!   `open_within_days=<days>`, `sort=created_at`, `order=desc`, `limit`, `offset`,
//!   and an identifying `User-Agent`.
//! - Uses `open_within_days` + `sort=created_at&order=desc` (rather than
//!   `posted_within_days` / `sort=posted_at`) because source boards frequently
//!   restate `posted_at` on every crawl ("fake freshness"). `open_within_days`
//!   filters on Freehire's immutable first-seen timestamp (`created_at`), and
//!   results are additionally verified and sorted newest-first client-side.
//! - Combines up to 5 active role titles into a single `title:"..." OR ...` query
//!   so each page turn is 1 lightweight request with exact pagination and `meta.total`.
//! - Hardened reliability:
//!   - Progressive multi-attempt retry on transient network/connect/TLS/stream/5xx/408/429
//!     failures (honoring `Retry-After` up to 4s) with fresh sockets (`Connection: close`)
//!     on retries to bypass stale Cloudflare/NAT pooled connections.
//!   - Never silently discards body-read errors into JSON parse failures.
//!   - Automatic fallback to the lighter `/jobs/search` endpoint when `/agent/jobs/search`
//!     is unsupported or repeatedly times out / returns 5xx.
//!   - Automatic fallback to simpler broad query if strict `title:"..."` syntax returns 400/422.
//! - All response structs use `#[serde(default)]` so upstream schema additions
//!   or omitted optional fields never break deserialization.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;
use url::Url;

use crate::jobs::models::{
    ExternalSearchLink, JobFilters, JobListing, JobSearchResult, MAX_PAGE_NUMBER, SearchProfile,
    format_boolean_term,
};

pub const FREEHIRE_API_BASE_URL: &str = "https://freehire.me/api/v1";
pub const FREEHIRE_WEB_BASE_URL: &str = "https://freehire.me";
const FREEHIRE_USER_AGENT: &str = concat!("vanguardcv/vanguard-cv/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT_SECS: u64 = 5;
const REQUEST_TIMEOUT_SECS: u64 = 18;
const DOH_TIMEOUT_SECS: u64 = 4;
const PRIMARY_MAX_ATTEMPTS: usize = 3;
const FALLBACK_MAX_ATTEMPTS: usize = 2;
const MAX_RETRY_AFTER_SECS: u64 = 4;

/// Pluggable job discovery provider abstraction.
#[async_trait]
pub trait JobProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn search(
        &self,
        profiles: &[SearchProfile],
        filters: &JobFilters,
    ) -> Result<JobSearchResult>;
}

/// Public, keyless Freehire.me HTTP JSON API client.
#[derive(Debug, Clone)]
pub struct FreehireProvider {
    http: Client,
    /// Cached alternate-edge HTTPS client resolved via DNS-over-HTTPS when local
    /// ISP DNS returns a blackholed Cloudflare edge IP (e.g. `188.114.96.x`).
    fallback_http: Arc<RwLock<Option<Client>>>,
    base_url: String,
    retry_base_delay: Duration,
}

#[derive(Debug, Deserialize)]
struct DohResponse {
    #[serde(rename = "Answer", default)]
    answer: Vec<DohAnswer>,
}

#[derive(Debug, Deserialize)]
struct DohAnswer {
    #[serde(rename = "type", default)]
    record_type: u16,
    #[serde(default)]
    data: String,
}

impl Default for FreehireProvider {
    fn default() -> Self {
        Self::new(FREEHIRE_API_BASE_URL)
    }
}

enum EndpointOutcome {
    Ok(FreehireSearchResponse),
    /// Endpoint returned 404/405/501 — safe to try `/jobs/search` fallback.
    Unsupported(anyhow::Error),
    /// TCP/TLS connect failure or fast connect timeout before reaching the server.
    Unreachable(anyhow::Error),
    /// Transient server/stream/408/429/5xx error after connecting — eligible for retry.
    Transient {
        error: anyhow::Error,
        retry_after: Option<Duration>,
    },
    /// Query syntax rejected by server (400 / 422) — caller may retry with a simpler query.
    BadQuery(anyhow::Error),
    /// Non-retryable client or permanent schema error.
    Fatal(anyhow::Error),
}

enum SearchQueryOutcome {
    Ok(FreehireSearchResponse, bool),
    Unreachable(anyhow::Error),
    Failed(anyhow::Error),
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    let secs = raw.parse::<u64>().ok()?;
    Some(Duration::from_secs(secs.min(MAX_RETRY_AFTER_SECS)))
}

impl FreehireProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_retry_base_delay(base_url, Duration::from_millis(450))
    }

    fn build_http_client(resolve_override: Option<(&str, &[SocketAddr])>) -> Client {
        let mut builder = Client::builder()
            .user_agent(FREEHIRE_USER_AGENT)
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .tcp_keepalive(Duration::from_secs(20))
            .pool_idle_timeout(Duration::from_secs(25))
            .pool_max_idle_per_host(2);
        if let Some((host, addrs)) = resolve_override {
            builder = builder.resolve_to_addrs(host, addrs);
        }
        builder.build().unwrap_or_else(|_| Client::new())
    }

    pub fn with_retry_base_delay(base_url: impl Into<String>, retry_base_delay: Duration) -> Self {
        Self {
            http: Self::build_http_client(None),
            fallback_http: Arc::new(RwLock::new(None)),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            retry_base_delay,
        }
    }

    /// Resolves alternate IPv4 edge addresses via Google DNS-over-HTTPS (`dns.google`)
    /// while keeping full HTTPS SNI & TLS certificate verification for the target host.
    async fn resolve_alternate_edge_client(&self, target_url: &Url) -> Option<Client> {
        if target_url.scheme() != "https" {
            return None;
        }
        let host = target_url.host_str()?;
        if host.eq_ignore_ascii_case("localhost") || host.parse::<IpAddr>().is_ok() {
            return None;
        }
        let port = target_url.port_or_known_default().unwrap_or(443);

        let system_ips: HashSet<IpAddr> =
            match tokio::time::timeout(Duration::from_secs(2), tokio::net::lookup_host((host, port)))
                .await
            {
                Ok(Ok(iter)) => iter.map(|sa| sa.ip()).collect(),
                _ => HashSet::new(),
            };

        let doh_client = Client::builder()
            .user_agent(FREEHIRE_USER_AGENT)
            .connect_timeout(Duration::from_secs(DOH_TIMEOUT_SECS))
            .timeout(Duration::from_secs(DOH_TIMEOUT_SECS))
            .build()
            .ok()?;

        let mut doh_url = Url::parse("https://dns.google/resolve").ok()?;
        doh_url
            .query_pairs_mut()
            .append_pair("name", host)
            .append_pair("type", "A");

        let resp = doh_client
            .get(doh_url)
            .header("Accept", "application/dns-json")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let parsed: DohResponse = resp.json().await.ok()?;
        let mut all_v4: Vec<SocketAddr> = Vec::new();
        let mut alternate_v4: Vec<SocketAddr> = Vec::new();
        for ans in parsed.answer {
            if ans.record_type != 1 {
                continue;
            }
            if let Ok(IpAddr::V4(v4)) = ans.data.trim().parse::<IpAddr>()
                && !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_unspecified()
                && !v4.is_broadcast()
                && !v4.is_multicast()
            {
                let sa = SocketAddr::new(IpAddr::V4(v4), port);
                if !all_v4.contains(&sa) {
                    all_v4.push(sa);
                    if !system_ips.contains(&IpAddr::V4(v4)) {
                        alternate_v4.push(sa);
                    }
                }
            }
        }

        let chosen = if !alternate_v4.is_empty() {
            alternate_v4
        } else {
            all_v4
        };
        if chosen.is_empty() {
            return None;
        }

        tracing::info!(
            host,
            addrs = ?chosen,
            "Resolved alternate HTTPS edge addresses via DNS-over-HTTPS"
        );
        let client = Self::build_http_client(Some((host, &chosen)));
        *self.fallback_http.write().await = Some(client.clone());
        Some(client)
    }

    fn backoff_for_attempt(&self, attempt_idx: usize, retry_after: Option<Duration>) -> Duration {
        if let Some(server_delay) = retry_after {
            return server_delay;
        }
        let multiplier = match attempt_idx {
            0 => 1,
            1 => 3,
            _ => 5,
        };
        self.retry_base_delay
            .saturating_mul(multiplier)
            .min(Duration::from_secs(3))
    }

    async fn send_with_client(
        client: &Client,
        url: &Url,
        force_fresh_connection: bool,
    ) -> std::result::Result<reqwest::Response, EndpointOutcome> {
        let mut req = client
            .get(url.as_str())
            .header("Accept", "application/json");
        if force_fresh_connection {
            req = req.header("Connection", "close");
        }
        let started = Instant::now();
        match req.send().await {
            Ok(r) => Ok(r),
            Err(err) => {
                let is_connect_failure = err.is_connect()
                    || (err.is_timeout()
                        && started.elapsed()
                            < Duration::from_secs(CONNECT_TIMEOUT_SECS.saturating_add(4)));
                if is_connect_failure {
                    Err(EndpointOutcome::Unreachable(anyhow!(
                        "Could not connect to Freehire.me (connection timed out). Check your internet connection or try again shortly."
                    )))
                } else if err.is_timeout() {
                    Err(EndpointOutcome::Transient {
                        error: anyhow!("Job search timed out waiting for results. Please try again shortly."),
                        retry_after: None,
                    })
                } else {
                    Err(EndpointOutcome::Transient {
                        error: anyhow!("Job search network error: {err}"),
                        retry_after: None,
                    })
                }
            }
        }
    }

    async fn query_endpoint_once(
        &self,
        endpoint_path: &str,
        query: &str,
        filters: &JobFilters,
        limit: usize,
        offset: usize,
        force_fresh_connection: bool,
    ) -> EndpointOutcome {
        let mut url = match Url::parse(&format!("{}{}", self.base_url, endpoint_path)) {
            Ok(u) => u,
            Err(err) => return EndpointOutcome::Fatal(anyhow!("Invalid job search URL: {err}")),
        };
        {
            let mut pairs = url.query_pairs_mut();
            let trimmed_q = query.trim();
            if !trimmed_q.is_empty() {
                pairs.append_pair("q", trimmed_q);
            }
            if endpoint_path.contains("/agent/") {
                pairs.append_pair("description_format", "markdown");
            }
            if filters.remote_only {
                pairs.append_pair("work_mode", "remote");
            }
            if let Some(country_code) = filters
                .country
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
            {
                pairs.append_pair("countries", &country_code.to_ascii_lowercase());
            }
            if let Some(days) = filters.posted_within_days
                && days > 0
            {
                // Use `open_within_days` (immutable first-seen date in Freehire's catalogue)
                // so boards that restate `posted_at` on every crawl cannot slip stale jobs
                // into narrow freshness windows such as 24h.
                pairs.append_pair("open_within_days", &days.to_string());
            }
            // Always order newest-first by immutable catalogue discovery time.
            pairs.append_pair("sort", "created_at");
            pairs.append_pair("order", "desc");

            let clamped_limit = limit.clamp(1, 20);
            pairs.append_pair("limit", &clamped_limit.to_string());
            pairs.append_pair("offset", &offset.to_string());
        }

        let cached_fallback = self.fallback_http.read().await.clone();
        let resp = if let Some(ref alt_client) = cached_fallback {
            match Self::send_with_client(alt_client, &url, force_fresh_connection).await {
                Ok(r) => r,
                Err(EndpointOutcome::Unreachable(_)) => {
                    *self.fallback_http.write().await = None;
                    match Self::send_with_client(&self.http, &url, true).await {
                        Ok(r) => r,
                        Err(EndpointOutcome::Unreachable(sys_err)) => {
                            if let Some(fresh_alt) = self.resolve_alternate_edge_client(&url).await {
                                match Self::send_with_client(&fresh_alt, &url, true).await {
                                    Ok(r) => r,
                                    Err(outcome) => return outcome,
                                }
                            } else {
                                return EndpointOutcome::Unreachable(sys_err);
                            }
                        }
                        Err(outcome) => return outcome,
                    }
                }
                Err(outcome) => return outcome,
            }
        } else {
            match Self::send_with_client(&self.http, &url, force_fresh_connection).await {
                Ok(r) => r,
                Err(EndpointOutcome::Unreachable(sys_err)) => {
                    tracing::warn!(
                        endpoint = endpoint_path,
                        "Primary DNS route unreachable ({sys_err}); attempting alternate HTTPS edge via DoH"
                    );
                    if let Some(alt_client) = self.resolve_alternate_edge_client(&url).await {
                        match Self::send_with_client(&alt_client, &url, true).await {
                            Ok(r) => r,
                            Err(outcome) => return outcome,
                        }
                    } else {
                        return EndpointOutcome::Unreachable(sys_err);
                    }
                }
                Err(outcome) => return outcome,
            }
        };

        let status = resp.status();
        let retry_after = parse_retry_after(resp.headers());
        let body = match resp.text().await {
            Ok(text) => text,
            Err(err) => {
                return EndpointOutcome::Transient {
                    error: anyhow!("Job search response stream interrupted: {err}"),
                    retry_after: None,
                };
            }
        };

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return EndpointOutcome::Transient {
                error: anyhow!(
                    "Job search rate limit reached (HTTP 429). Wait a moment before refreshing."
                ),
                retry_after: Some(retry_after.unwrap_or_else(|| Duration::from_secs(2))),
            };
        }
        if status == reqwest::StatusCode::REQUEST_TIMEOUT {
            return EndpointOutcome::Transient {
                error: anyhow!("Job search request timed out (HTTP 408)."),
                retry_after,
            };
        }
        if matches!(
            status,
            reqwest::StatusCode::NOT_FOUND
                | reqwest::StatusCode::METHOD_NOT_ALLOWED
                | reqwest::StatusCode::NOT_IMPLEMENTED
        ) {
            return EndpointOutcome::Unsupported(anyhow!(
                "Job search endpoint unavailable (HTTP {}).",
                status.as_u16()
            ));
        }
        if status.is_server_error() || matches!(status.as_u16(), 520..=530) {
            return EndpointOutcome::Transient {
                error: anyhow!(
                    "Job search service temporarily unavailable (HTTP {}).",
                    status.as_u16()
                ),
                retry_after,
            };
        }
        if !status.is_success() {
            let api_msg = serde_json::from_str::<FreehireErrorEnvelope>(&body)
                .ok()
                .and_then(|e| e.error.or(e.detail))
                .filter(|s| !s.trim().is_empty());
            let err = match api_msg {
                Some(msg) => anyhow!("Job search error (HTTP {}): {msg}", status.as_u16()),
                None => anyhow!("Job search returned HTTP {}.", status.as_u16()),
            };
            if matches!(
                status,
                reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY
            ) {
                return EndpointOutcome::BadQuery(err);
            }
            return EndpointOutcome::Fatal(err);
        }

        let trimmed_body = body.trim();
        if trimmed_body.is_empty() || trimmed_body.starts_with('<') {
            // Empty body or unexpected HTML edge/interstitial page on HTTP 200 — retry cleanly.
            return EndpointOutcome::Transient {
                error: anyhow!("Job search service returned an incomplete response."),
                retry_after: None,
            };
        }

        match serde_json::from_str::<FreehireSearchResponse>(trimmed_body)
            .context("Failed to parse job search JSON response")
        {
            Ok(parsed) => {
                if let Some(meta) = parsed.meta.as_ref()
                    && !meta.ignored_params.is_empty()
                {
                    tracing::warn!(
                        ignored_params = ?meta.ignored_params,
                        "Freehire search API reported ignored query parameters"
                    );
                }
                EndpointOutcome::Ok(parsed)
            }
            Err(err) if trimmed_body.starts_with('{') && !trimmed_body.ends_with('}') => {
                // Truncated JSON payload from an interrupted connection.
                EndpointOutcome::Transient {
                    error: err,
                    retry_after: None,
                }
            }
            Err(err) => EndpointOutcome::Fatal(err),
        }
    }

    async fn query_endpoint_with_retries(
        &self,
        endpoint_path: &str,
        query: &str,
        filters: &JobFilters,
        limit: usize,
        offset: usize,
        max_attempts: usize,
    ) -> EndpointOutcome {
        let mut last_transient: Option<anyhow::Error> = None;
        for attempt in 0..max_attempts {
            let force_fresh = attempt > 0;
            match self
                .query_endpoint_once(endpoint_path, query, filters, limit, offset, force_fresh)
                .await
            {
                EndpointOutcome::Ok(resp) => return EndpointOutcome::Ok(resp),
                EndpointOutcome::Unsupported(err) => return EndpointOutcome::Unsupported(err),
                EndpointOutcome::Unreachable(err) => return EndpointOutcome::Unreachable(err),
                EndpointOutcome::BadQuery(err) => return EndpointOutcome::BadQuery(err),
                EndpointOutcome::Fatal(err) => return EndpointOutcome::Fatal(err),
                EndpointOutcome::Transient { error, retry_after } => {
                    let is_last = attempt + 1 >= max_attempts;
                    if is_last {
                        last_transient = Some(error);
                        break;
                    }
                    let delay = self.backoff_for_attempt(attempt, retry_after);
                    tracing::info!(
                        endpoint = endpoint_path,
                        attempt = attempt + 1,
                        delay_ms = delay.as_millis() as u64,
                        "Transient job search error ({error}); retrying with fresh connection"
                    );
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    last_transient = Some(error);
                }
            }
        }
        EndpointOutcome::Transient {
            error: last_transient
                .unwrap_or_else(|| anyhow!("Job search failed after retries. Please try again.")),
            retry_after: None,
        }
    }

    async fn query_with_polite_retry(
        &self,
        query: &str,
        filters: &JobFilters,
        limit: usize,
        offset: usize,
    ) -> SearchQueryOutcome {
        match self
            .query_endpoint_with_retries(
                "/agent/jobs/search",
                query,
                filters,
                limit,
                offset,
                PRIMARY_MAX_ATTEMPTS,
            )
            .await
        {
            EndpointOutcome::Ok(resp) => SearchQueryOutcome::Ok(resp, false),
            EndpointOutcome::Unreachable(err) => SearchQueryOutcome::Unreachable(err),
            EndpointOutcome::BadQuery(err) | EndpointOutcome::Fatal(err) => {
                SearchQueryOutcome::Failed(err)
            }
            EndpointOutcome::Unsupported(orig_err) => {
                tracing::warn!(
                    "Primary job search endpoint unsupported ({orig_err}); trying standard /jobs/search endpoint"
                );
                match self
                    .query_endpoint_with_retries(
                        "/jobs/search",
                        query,
                        filters,
                        limit,
                        offset,
                        FALLBACK_MAX_ATTEMPTS,
                    )
                    .await
                {
                    EndpointOutcome::Ok(resp) => SearchQueryOutcome::Ok(resp, true),
                    EndpointOutcome::Unreachable(e) => SearchQueryOutcome::Unreachable(e),
                    EndpointOutcome::Fatal(e)
                    | EndpointOutcome::BadQuery(e)
                    | EndpointOutcome::Unsupported(e)
                    | EndpointOutcome::Transient { error: e, .. } => SearchQueryOutcome::Failed(e),
                }
            }
            EndpointOutcome::Transient {
                error: primary_err, ..
            } => {
                tracing::warn!(
                    "Primary /agent/jobs/search failed after {PRIMARY_MAX_ATTEMPTS} attempts ({primary_err}); falling back to /jobs/search"
                );
                match self
                    .query_endpoint_with_retries(
                        "/jobs/search",
                        query,
                        filters,
                        limit,
                        offset,
                        FALLBACK_MAX_ATTEMPTS,
                    )
                    .await
                {
                    EndpointOutcome::Ok(resp) => SearchQueryOutcome::Ok(resp, true),
                    EndpointOutcome::Unreachable(e) => SearchQueryOutcome::Unreachable(e),
                    EndpointOutcome::Fatal(_)
                    | EndpointOutcome::BadQuery(_)
                    | EndpointOutcome::Unsupported(_)
                    | EndpointOutcome::Transient { .. } => SearchQueryOutcome::Failed(primary_err),
                }
            }
        }
    }
}

/// Builds a Freehire-optimized combined query from active search profiles.
///
/// Uses Freehire OpenAPI 3.0.3 `title:"Role A" OR title:"Role B"` syntax (up to 5
/// alternatives) so all active profiles execute in a single fast request with exact
/// pagination and without noisy description-only matches.
///
/// Returns `(primary_query, optional_broader_fallback_query)`.
pub fn build_freehire_query(
    profiles: &[SearchProfile],
    manual_query: &str,
) -> (String, Option<String>) {
    let manual = manual_query.trim();
    if !manual.is_empty() {
        return (manual.to_string(), None);
    }

    let active: Vec<&SearchProfile> = profiles.iter().filter(|p| p.enabled).take(5).collect();
    if active.is_empty() {
        return (String::new(), None);
    }

    let title_clauses: Vec<String> = active
        .iter()
        .filter_map(|p| p.to_freehire_title_clause())
        .collect();

    if title_clauses.is_empty() {
        return (String::new(), None);
    }

    let primary = title_clauses.join(" OR ");

    // Broader fallback (only used if strict `title:` returned 0 hits on page 1 or 400):
    // drop `title:` prefix so Freehire searches title + description for the role titles.
    let broad_terms: Vec<String> = active
        .iter()
        .map(|p| p.broad_title_query())
        .filter(|t| !t.is_empty())
        .map(|t| format_boolean_term(&t))
        .collect();
    let broad = if broad_terms.is_empty() {
        None
    } else {
        let joined = broad_terms.join(" OR ");
        (joined != primary).then_some(joined)
    };

    (primary, broad)
}

fn infer_matched_profile(raw: &FreehireJob, active_profiles: &[&SearchProfile]) -> Option<String> {
    if active_profiles.is_empty() {
        return None;
    }
    if active_profiles.len() == 1 {
        return Some(active_profiles[0].title.clone());
    }
    let job_title_lower = raw.title.to_ascii_lowercase();
    let job_skills_lower: Vec<String> = raw.skills.iter().map(|s| s.to_ascii_lowercase()).collect();

    let mut best_title: Option<&str> = None;
    let mut best_score = 0usize;

    for prof in active_profiles {
        let mut score = 0usize;
        let prof_lower = prof.title.to_ascii_lowercase();
        if job_title_lower.contains(&prof_lower) {
            score += 10;
        } else {
            for word in prof_lower.split_whitespace().filter(|w| w.len() >= 3) {
                if job_title_lower.contains(word) {
                    score += 3;
                }
            }
        }
        for kw in prof.active_keywords() {
            let kw_lower = kw.to_ascii_lowercase();
            if job_skills_lower.iter().any(|s| s == &kw_lower)
                || job_title_lower.contains(&kw_lower)
            {
                score += 2;
            }
        }
        if score > best_score {
            best_score = score;
            best_title = Some(prof.title.as_str());
        }
    }

    best_title
        .map(str::to_string)
        .or_else(|| Some(active_profiles[0].title.clone()))
}

#[async_trait]
impl JobProvider for FreehireProvider {
    fn name(&self) -> &'static str {
        "Freehire.me"
    }

    async fn search(
        &self,
        profiles: &[SearchProfile],
        filters: &JobFilters,
    ) -> Result<JobSearchResult> {
        let page = filters.clamped_page();
        let page_size = filters.clamped_page_size();
        let offset = filters.offset();

        let (primary_q, broad_q) = build_freehire_query(profiles, &filters.manual_query);
        let active_profiles: Vec<&SearchProfile> = profiles.iter().filter(|p| p.enabled).collect();
        let is_custom = !filters.manual_query.trim().is_empty();

        let (mut resp, mut used_fallback_endpoint) = match self
            .query_with_polite_retry(&primary_q, filters, page_size, offset)
            .await
        {
            SearchQueryOutcome::Ok(r, fb) => (r, fb),
            SearchQueryOutcome::Unreachable(err) => return Err(err),
            SearchQueryOutcome::Failed(primary_err) => {
                if let Some(broad) = broad_q.as_deref()
                    && primary_q.contains("title:")
                {
                    tracing::info!(
                        "Primary title-scoped job query failed ({primary_err}); retrying with broader query"
                    );
                    match self
                        .query_with_polite_retry(broad, filters, page_size, offset)
                        .await
                    {
                        SearchQueryOutcome::Ok(r, fb) => (r, fb),
                        SearchQueryOutcome::Unreachable(err) => return Err(err),
                        SearchQueryOutcome::Failed(_) => return Err(primary_err),
                    }
                } else {
                    return Err(primary_err);
                }
            }
        };

        // If strict `title:` query had 0 hits on page 1, broaden once politely.
        if resp.data.is_empty()
            && page == 1
            && let Some(broad) = broad_q.as_deref()
            && let SearchQueryOutcome::Ok(broader, fb) = self
                .query_with_polite_retry(broad, filters, page_size, offset)
                .await
            && !broader.data.is_empty()
        {
            resp = broader;
            used_fallback_endpoint |= fb;
        }

        let mut total_estimated = resp.meta.as_ref().map(|m| m.total).unwrap_or(0);
        let raw_count = resp.data.len();

        let mut seen_slugs = HashSet::new();
        let mut jobs = Vec::with_capacity(raw_count);
        for raw_job in resp.data {
            let slug = raw_job.public_slug.trim().to_string();
            let dedup_key = if slug.is_empty() {
                format!(
                    "{}::{}",
                    raw_job.company.to_ascii_lowercase(),
                    raw_job.title.to_ascii_lowercase()
                )
            } else {
                slug
            };
            if !seen_slugs.insert(dedup_key) {
                continue;
            }
            let matched = if is_custom {
                Some("Custom search".to_string())
            } else {
                infer_matched_profile(&raw_job, &active_profiles)
            };
            jobs.push(normalize_freehire_job(raw_job, matched));
        }

        // Client-side freshness guard: never surface postings whose verified age exceeds
        // the selected freshness window (protects against upstream fake-freshness slips).
        if let Some(max_days) = filters.posted_within_days
            && max_days > 0
        {
            let max_days_i64 = i64::from(max_days);
            jobs.retain(|job| job.age_days.is_none_or(|d| d <= max_days_i64));
        }

        // Always sort strictly newest-first: smallest `age_days` first (matching the visible
        // card age badge), then newest RFC3339 timestamp first within the same day.
        jobs.sort_by(|a, b| {
            let a_age = a.age_days.unwrap_or(i64::MAX);
            let b_age = b.age_days.unwrap_or(i64::MAX);
            a_age.cmp(&b_age).then_with(|| {
                let a_ts = a.sort_timestamp.unwrap_or(i64::MIN);
                let b_ts = b.sort_timestamp.unwrap_or(i64::MIN);
                b_ts.cmp(&a_ts)
            })
        });

        let has_next_page = page < MAX_PAGE_NUMBER
            && raw_count > 0
            && ((offset + raw_count) as u64) < total_estimated;

        if page == 1 && !has_next_page && total_estimated == raw_count as u64 {
            total_estimated = jobs.len() as u64;
        }

        let warnings = if used_fallback_endpoint {
            vec![
                "Loaded from the standard search endpoint while the primary endpoint was busy."
                    .to_string(),
            ]
        } else {
            Vec::new()
        };

        Ok(JobSearchResult {
            jobs,
            total_estimated,
            page,
            page_size,
            has_next_page,
            from_cache: false,
            warnings,
        })
    }
}

// ---------------------------------------------------------------------------
// Defensive Freehire API wire structs (OpenAPI 3.0.3)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct FreehireErrorEnvelope {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FreehireSearchResponse {
    #[serde(default)]
    data: Vec<FreehireJob>,
    #[serde(default)]
    meta: Option<FreehireMeta>,
}

#[derive(Debug, Default, Deserialize)]
struct FreehireMeta {
    #[serde(default)]
    total: u64,
    #[serde(default)]
    ignored_params: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FreehireJob {
    #[serde(default)]
    public_slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    company: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    direct_apply: Option<bool>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    work_mode: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    regions: Vec<String>,
    #[serde(default)]
    countries: Vec<String>,
    #[serde(default)]
    cities: Vec<String>,
    #[serde(default)]
    posted_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    enrichment: Option<FreehireEnrichment>,
    #[serde(default)]
    reality: Option<FreehireReality>,
}

#[derive(Debug, Default, Deserialize)]
struct FreehireEnrichment {
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    employment_type: Option<String>,
    #[serde(default)]
    salary_min: Option<i64>,
    #[serde(default)]
    salary_max: Option<i64>,
    #[serde(default)]
    salary_currency: Option<String>,
    #[serde(default)]
    salary_period: Option<String>,
    #[serde(default)]
    seniority: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FreehireReality {
    #[serde(default)]
    age_days: Option<i64>,
}

fn parse_rfc3339_timestamp(raw: Option<&str>) -> Option<i64> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
}

fn normalize_freehire_job(raw: FreehireJob, matched_profile: Option<String>) -> JobListing {
    let freehire_url = if raw.public_slug.trim().is_empty() {
        FREEHIRE_WEB_BASE_URL.to_string()
    } else {
        format!("{FREEHIRE_WEB_BASE_URL}/jobs/{}", raw.public_slug.trim())
    };

    let apply_url = if raw.direct_apply == Some(true) {
        freehire_url.clone()
    } else {
        raw.url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| freehire_url.clone())
    };

    let location = raw
        .location
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .or_else(|| {
            if !raw.cities.is_empty() {
                Some(raw.cities.join(", "))
            } else if !raw.countries.is_empty() {
                Some(
                    raw.countries
                        .iter()
                        .map(|c| c.to_ascii_uppercase())
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            } else if !raw.regions.is_empty() {
                Some(
                    raw.regions
                        .iter()
                        .map(|r| r.replace('_', " "))
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            if raw.work_mode.as_deref() == Some("remote") {
                "Remote".to_string()
            } else {
                "Location unspecified".to_string()
            }
        });

    let enrichment = raw.enrichment.unwrap_or_default();
    let salary_display = format_salary(
        enrichment.salary_min,
        enrichment.salary_max,
        enrichment.salary_currency.as_deref(),
        enrichment.salary_period.as_deref(),
    );

    let description_markdown = raw
        .description
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    let preview = enrichment
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| summarize_markdown_preview(&description_markdown, 260));

    // Prefer immutable `created_at` (when Freehire first recorded the role) over
    // source-supplied `posted_at` for chronological ordering.
    let sort_timestamp = parse_rfc3339_timestamp(raw.created_at.as_deref())
        .or_else(|| parse_rfc3339_timestamp(raw.posted_at.as_deref()));

    let age_days = raw
        .reality
        .and_then(|r| r.age_days)
        .map(|d| d.max(0))
        .or_else(|| {
            sort_timestamp.map(|ts| {
                let elapsed_secs = (Utc::now().timestamp() - ts).max(0);
                elapsed_secs / 86_400
            })
        });

    let age_label = age_days.map(|days| {
        if days <= 0 {
            "Today".to_string()
        } else if days == 1 {
            "1d ago".to_string()
        } else {
            format!("{days}d ago")
        }
    });

    JobListing {
        public_slug: raw.public_slug,
        title: if raw.title.trim().is_empty() {
            "Untitled role".to_string()
        } else {
            raw.title.trim().to_string()
        },
        company: if raw.company.trim().is_empty() {
            "Confidential employer".to_string()
        } else {
            raw.company.trim().to_string()
        },
        location,
        work_mode: raw
            .work_mode
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        salary_display,
        seniority: enrichment
            .seniority
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        employment_type: enrichment
            .employment_type
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        skills: raw.skills,
        preview,
        description_markdown,
        apply_url,
        freehire_url,
        posted_at: raw.created_at.or(raw.posted_at),
        age_days,
        sort_timestamp,
        age_label,
        source: raw.source,
        matched_profile,
    }
}

fn format_salary(
    min: Option<i64>,
    max: Option<i64>,
    currency: Option<&str>,
    period: Option<&str>,
) -> Option<String> {
    let min = min.filter(|v| *v > 0);
    let max = max.filter(|v| *v > 0);
    if min.is_none() && max.is_none() {
        return None;
    }
    let cur = currency
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("USD")
        .to_ascii_uppercase();
    let suffix = match period.map(str::trim) {
        Some("year") | Some("annual") | Some("yearly") | None => "/yr",
        Some("month") | Some("monthly") => "/mo",
        Some("hour") | Some("hourly") => "/hr",
        Some("day") | Some("daily") => "/day",
        Some(_) => "",
    };
    let fmt_amt = |n: i64| -> String {
        if (n >= 1000 && n % 1000 == 0) || n >= 10_000 {
            format!("{}k", n / 1000)
        } else {
            n.to_string()
        }
    };
    let range = match (min, max) {
        (Some(lo), Some(hi)) if lo != hi => format!("{}-{}", fmt_amt(lo), fmt_amt(hi)),
        (Some(lo), _) => fmt_amt(lo),
        (None, Some(hi)) => fmt_amt(hi),
        (None, None) => return None,
    };
    Some(format!("{cur} {range}{suffix}"))
}

fn summarize_markdown_preview(markdown: &str, max_chars: usize) -> String {
    let cleaned: String = markdown
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(['#', '*', '-', '>', '`'])
                .replace("**", "")
                .replace("__", "")
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let normalized = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        let mut out: String = normalized.chars().take(max_chars).collect();
        if let Some(last_space) = out.rfind(' ')
            && last_space > max_chars / 2
        {
            out.truncate(last_space);
        }
        out.push('…');
        out
    }
}

/// Builds platform-optimized Boolean queries for LinkedIn and Indeed.
fn build_board_queries(profiles: &[SearchProfile], manual_query: &str) -> (String, String) {
    let manual = manual_query.trim();
    if !manual.is_empty() {
        return (manual.to_string(), manual.to_string());
    }
    let active: Vec<&SearchProfile> = profiles.iter().filter(|p| p.enabled).collect();
    if active.is_empty() {
        return (String::new(), String::new());
    }
    if active.len() == 1 {
        return (active[0].to_linkedin_query(), active[0].to_indeed_query());
    }

    // Multiple active profiles: combine up to 3 role titles with OR.
    // - LinkedIn handles grouped Boolean `(Role1 OR Role2) AND (Skill1 OR Skill2)` reliably.
    // - Indeed's consumer search strips operators when given adjacent multi-group parens or
    //   long strings, so keep multi-role Indeed queries as a clean top-level OR of role titles
    //   (`"Role 1" OR "Role 2" OR "Role 3"`). Single-profile searches add `AND (Skill1 OR Skill2)`.
    let titles: Vec<String> = active
        .iter()
        .take(3)
        .map(|p| p.broad_title_query())
        .filter(|t| !t.is_empty())
        .map(|t| format_boolean_term(&t))
        .collect();

    let mut seen_kw = HashSet::new();
    let mut skills = Vec::new();
    for prof in &active {
        for kw in prof.distinct_active_keywords(2) {
            if seen_kw.insert(kw.to_ascii_lowercase()) {
                skills.push(format_boolean_term(&kw));
                if skills.len() >= 2 {
                    break;
                }
            }
        }
        if skills.len() >= 2 {
            break;
        }
    }

    let title_group = match titles.len() {
        0 => String::new(),
        1 => titles[0].clone(),
        _ => format!("({})", titles.join(" OR ")),
    };
    let skill_group = match skills.len() {
        0 => String::new(),
        1 => skills[0].clone(),
        _ => format!("({})", skills.join(" OR ")),
    };

    let linkedin = match (title_group.is_empty(), skill_group.is_empty()) {
        (false, false) => format!("{title_group} AND {skill_group}"),
        (false, true) => title_group,
        (true, false) => skill_group.clone(),
        (true, true) => String::new(),
    };
    let indeed = if !titles.is_empty() {
        titles.join(" OR ")
    } else {
        skills.join(" OR ")
    };

    (linkedin, indeed)
}

/// Builds optimized quick-search deep links for LinkedIn and Indeed matching the
/// active search profiles, selected country/location, remote filter, and freshness window.
///
/// Deliberately omits any direct Freehire.me web search URL.
pub fn build_external_search_links(
    profiles: &[SearchProfile],
    filters: &JobFilters,
) -> Vec<ExternalSearchLink> {
    let (linkedin_q, indeed_q) = build_board_queries(profiles, &filters.manual_query);
    if linkedin_q.is_empty() && indeed_q.is_empty() {
        return Vec::new();
    }
    let selected_country = filters.selected_country();
    let mut links = Vec::new();

    if !linkedin_q.is_empty()
        && let Ok(mut u) = Url::parse("https://www.linkedin.com/jobs/search/")
    {
        {
            let mut pairs = u.query_pairs_mut();
            pairs.append_pair("keywords", &linkedin_q);
            if let Some(country) = selected_country {
                pairs.append_pair("location", country.name);
            }
            if filters.remote_only {
                pairs.append_pair("f_WT", "2");
            }
            if let Some(days) = filters.posted_within_days
                && days > 0
            {
                let seconds = (days.clamp(1, 30) as u64) * 86_400;
                pairs.append_pair("f_TPR", &format!("r{seconds}"));
            }
            pairs.append_pair("sortBy", "DD");
        }
        links.push(ExternalSearchLink {
            platform: "LinkedIn",
            url: u.to_string(),
        });
    }

    let indeed_host = selected_country
        .map(|c| c.indeed_host)
        .unwrap_or("www.indeed.com");
    if !indeed_q.is_empty()
        && let Ok(mut u) = Url::parse(&format!("https://{indeed_host}/jobs"))
    {
        {
            let mut pairs = u.query_pairs_mut();
            pairs.append_pair("q", &indeed_q);
            if filters.remote_only {
                if let Some(country) = selected_country
                    && country.indeed_host == "www.indeed.com"
                    && country.code != "us"
                {
                    // Country without its own Indeed subdomain: preserve country in `l`
                    // and apply Indeed's remote attribute filter.
                    pairs.append_pair("l", country.name);
                    pairs.append_pair("sc", "0kf:attr(DSQF7);");
                } else {
                    pairs.append_pair("l", "Remote");
                }
            } else if let Some(country) = selected_country {
                pairs.append_pair("l", country.name);
            }
            if let Some(days) = filters.posted_within_days
                && days > 0
            {
                pairs.append_pair("fromage", &days.clamp(1, 30).to_string());
            }
            pairs.append_pair("sort", "date");
        }
        links.push(ExternalSearchLink {
            platform: "Indeed",
            url: u.to_string(),
        });
    }

    links
}
