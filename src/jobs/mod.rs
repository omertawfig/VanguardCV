//! Closed-loop Job Discovery subsystem.
//!
//! Keeps models, strictly PII-stripped AI search profile extraction, pluggable
//! job providers (Freehire.me public API + optimized LinkedIn/Indeed deep-link
//! builders), persistent per-CV profile caching, and session-only paginated
//! listing caching cleanly isolated from the UI layer.

pub mod cache;
pub mod extractor;
pub mod models;
pub mod provider;

pub use cache::{
    DEFAULT_LISTINGS_TTL, JobDiscoveryCache, MIN_FETCH_COOLDOWN, compute_cv_content_hash,
};
pub use extractor::{
    build_profile_extraction_prompts, build_scrubbed_digests, extract_fallback_profiles,
    generate_search_profiles_with_ai, parse_search_profiles_json,
};
pub use models::{
    CountryOption, CvSearchSource, DEFAULT_PAGE_SIZE, ExternalSearchLink, JOB_COUNTRIES,
    JobFilters, JobListing, JobSearchResult, MAX_PAGE_NUMBER, MAX_PAGE_SIZE, PersistedProfileEntry,
    SearchProfile, find_country_by_code, normalize_country_code,
};
pub use provider::{
    FREEHIRE_API_BASE_URL, FreehireProvider, JobProvider, build_external_search_links,
    build_freehire_query,
};

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::mocks::sample_root_document;
    use crate::privacy::scrubber::PiiScrubber;

    #[test]
    fn outbound_profile_prompt_contains_zero_pii() {
        let mut doc = sample_root_document();
        doc.pii_header.full_name = "Alice Secretname".to_string();
        doc.pii_header.email = Some("alice.secret@example.org".to_string());
        doc.pii_header.phone = Some("+49 170 555 9988".to_string());
        doc.pii_header.location = Some("Berlin, Germany".to_string());

        let scrubber = PiiScrubber;
        let digests = build_scrubbed_digests(&[&doc], &scrubber);
        let (sys, user) = build_profile_extraction_prompts(&digests);
        let combined = format!("{sys}\n{user}");

        assert!(!combined.contains("Alice Secretname"));
        assert!(!combined.contains("alice.secret@example.org"));
        assert!(!combined.contains("+49 170 555 9988"));
        assert!(!combined.contains("Berlin, Germany"));
        assert!(!combined.contains("[REDACTED_PII_"));
        assert!(
            !digests[0].role_titles.is_empty() || !digests[0].skills.is_empty(),
            "digest should carry non-PII roles/skills"
        );
    }

    #[test]
    fn parses_fenced_and_prose_wrapped_profile_json_and_falls_back_cleanly() {
        let fenced = r#"Here are your search profiles:
```json
{
  "profiles": [
    {
      "label": "Primary Role",
      "title": "Senior Security Engineer",
      "keywords": ["Penetration Testing", "AWS", "Rust", "Extra1", "Extra2"]
    },
    {
      "label": "Adjacent Role",
      "title": "Application Security Engineer",
      "keywords": ["AppSec", "Burp Suite", "AppSec"]
    }
  ]
}
```
"#;
        let parsed = parse_search_profiles_json(fenced).expect("should parse fenced JSON");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "Senior Security Engineer");
        assert_eq!(parsed[0].keywords.len(), 4); // capped at 4
        assert_eq!(parsed[1].keywords, vec!["AppSec", "Burp Suite"]); // deduped

        assert!(parse_search_profiles_json("totally invalid response").is_err());

        let doc = sample_root_document();
        let fallback = extract_fallback_profiles(&[&doc], &PiiScrubber);
        assert!(!fallback.is_empty());
        assert!(!fallback[0].title.is_empty());
    }

    #[test]
    fn persisted_profiles_roundtrip_and_invalidate_when_cv_changes() {
        let mut doc = sample_root_document();
        let source = CvSearchSource::Single(doc.id);
        let hash_v1 = compute_cv_content_hash(&[&doc]);

        let mut cache = JobDiscoveryCache::new(Duration::from_secs(60));
        let profs = vec![SearchProfile::new(
            "p1",
            "Primary Role",
            "Security Engineer",
            vec!["Rust".to_string()],
        )];
        cache.put_profiles(&source, &hash_v1, profs, false);

        // Simulate persisting to SQLite settings and restoring in a new session.
        let json = cache
            .serialize_persisted_profiles()
            .expect("serialize profiles");
        let mut restored = JobDiscoveryCache::new(Duration::from_secs(60));
        restored.load_persisted_profiles(&json);
        let (hit, used_fb) = restored
            .get_profiles(&source, &hash_v1)
            .expect("persisted hit");
        assert_eq!(hit.len(), 1);
        assert!(!used_fb);

        // Editing CV body changes content hash -> cache miss + stale flag.
        doc.body.sections[0].title.push_str(" Updated");
        let hash_v2 = compute_cv_content_hash(&[&doc]);
        assert_ne!(hash_v1, hash_v2);
        assert!(restored.get_profiles(&source, &hash_v2).is_none());
        assert!(restored.has_stale_profiles(&source, &hash_v2));

        // Listings cache + manual invalidation
        restored.put_listings(
            "sig-1",
            JobSearchResult {
                jobs: Vec::new(),
                total_estimated: 42,
                page: 1,
                page_size: 10,
                has_next_page: true,
                from_cache: false,
                warnings: Vec::new(),
            },
        );
        let listing_hit = restored.get_listings("sig-1").expect("cached hit");
        assert!(listing_hit.from_cache);
        assert_eq!(listing_hit.total_estimated, 42);

        restored.invalidate_listings();
        assert!(restored.get_listings("sig-1").is_none());
    }

    #[test]
    fn platform_queries_embed_timing_and_country_location_across_linkedin_and_indeed() {
        let profiles = vec![
            SearchProfile::new(
                "p1",
                "Primary Role",
                "Security Researcher",
                vec!["Rust".to_string(), "Penetration Testing".to_string()],
            ),
            SearchProfile::new(
                "p2",
                "Adjacent Role",
                "Detection Engineer",
                vec!["SIEM".to_string(), "Python".to_string()],
            ),
        ];

        // Freehire combines into 1 single `title:` OR query.
        let (fh_q, fh_broad) = build_freehire_query(&profiles, "");
        assert_eq!(
            fh_q,
            "title:\"Security Researcher\" OR title:\"Detection Engineer\""
        );
        assert!(fh_broad.is_some());

        // Country = Germany, Remote = true, Freshness = 24h
        let filters = JobFilters {
            remote_only: true,
            country: Some("de".to_string()),
            posted_within_days: Some(1),
            manual_query: String::new(),
            page: 1,
            page_size: 10,
        };
        let links = build_external_search_links(&profiles, &filters);
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.platform != "Freehire.me"));

        let linkedin = links.iter().find(|l| l.platform == "LinkedIn").unwrap();
        let parsed_li = url::Url::parse(&linkedin.url).unwrap();
        let li_param = |k: &str| {
            parsed_li
                .query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(li_param("location").as_deref(), Some("Germany"));
        assert_eq!(li_param("f_WT").as_deref(), Some("2"));
        assert_eq!(li_param("f_TPR").as_deref(), Some("r86400"));
        assert_eq!(li_param("sortBy").as_deref(), Some("DD"));
        assert!(li_param("keywords").unwrap().contains("AND"));

        let indeed = links.iter().find(|l| l.platform == "Indeed").unwrap();
        let parsed_indeed = url::Url::parse(&indeed.url).unwrap();
        assert_eq!(parsed_indeed.host_str(), Some("de.indeed.com"));
        let in_param = |k: &str| {
            parsed_indeed
                .query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(in_param("fromage").as_deref(), Some("1"));
        assert_eq!(in_param("sort").as_deref(), Some("date"));
        assert_eq!(in_param("l").as_deref(), Some("Remote"));
        // Multi-profile Indeed query stays clean top-level OR without broken `) (` groups.
        assert_eq!(
            in_param("q").as_deref(),
            Some("\"Security Researcher\" OR \"Detection Engineer\"")
        );

        // Non-remote country search embeds country name directly in Indeed `l=` and regional host
        let onsite_uk = JobFilters {
            remote_only: false,
            country: Some("gb".to_string()),
            posted_within_days: Some(7),
            manual_query: String::new(),
            page: 1,
            page_size: 10,
        };
        let uk_links = build_external_search_links(&profiles[..1], &onsite_uk);
        let uk_indeed = uk_links.iter().find(|l| l.platform == "Indeed").unwrap();
        let uk_parsed = url::Url::parse(&uk_indeed.url).unwrap();
        assert_eq!(uk_parsed.host_str(), Some("uk.indeed.com"));
        let uk_param = |k: &str| {
            uk_parsed
                .query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(uk_param("l").as_deref(), Some("United Kingdom"));
        assert_eq!(uk_param("fromage").as_deref(), Some("7"));
        assert_eq!(uk_param("sort").as_deref(), Some("date"));
        assert_eq!(
            uk_param("q").as_deref(),
            Some("\"Security Researcher\" AND (Rust OR \"Penetration Testing\")")
        );

        // Cache signature changes when country changes
        let mut other_country = onsite_uk.clone();
        other_country.country = Some("us".to_string());
        assert_ne!(
            onsite_uk.cache_signature(&profiles),
            other_country.cache_signature(&profiles)
        );
    }

    #[tokio::test]
    async fn freehire_provider_filters_stale_jobs_sorts_newest_first_and_sends_country() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            // Attempt 1: simulate cold-start transient 503
            let (mut s1, _) = listener.accept().await.expect("accept 1");
            let mut buf = vec![0u8; 4096];
            let _ = s1.read(&mut buf).await.expect("read 1");
            let resp_503 = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            s1.write_all(resp_503.as_bytes()).await.expect("write 1");

            // Attempt 2: polite retry succeeds with page 2 offset=10
            let (mut s2, _) = listener.accept().await.expect("accept 2");
            let n = s2.read(&mut buf).await.expect("read 2");
            let req = String::from_utf8_lossy(&buf[..n]).to_string();

            assert!(req.contains("GET /api/v1/agent/jobs/search?"));
            assert!(req.contains("description_format=markdown"));
            assert!(req.contains("work_mode=remote"));
            assert!(req.contains("countries=de"));
            assert!(req.contains("open_within_days=1"));
            assert!(req.contains("sort=created_at"));
            assert!(req.contains("order=desc"));
            assert!(req.contains("limit=10"));
            assert!(req.contains("offset=10"));
            assert!(req.to_ascii_lowercase().contains("user-agent: vanguardcv/"));

            // Intentionally return out-of-order jobs + 1 stale 18d-old fake-freshness job
            // to verify strict client-side freshness filtering and newest-first ordering.
            let body = r###"{
                "data": [
                    {
                        "public_slug": "older-yesterday-role",
                        "title": "AppSec Lead",
                        "company": "DirectCo",
                        "direct_apply": true,
                        "created_at": "2026-11-22T09:00:00Z",
                        "reality": { "age_days": 1 },
                        "description": "# Role\n**Own** security reviews."
                    },
                    {
                        "public_slug": "stale-fake-freshness-role",
                        "title": "GenAI Security Researcher",
                        "company": "StaleCorp",
                        "created_at": "2026-11-05T08:00:00Z",
                        "reality": { "age_days": 18 },
                        "description": "Reposted old listing."
                    },
                    {
                        "public_slug": "today-earlier-role",
                        "title": "Cloud Security Engineer",
                        "company": "BetaSec",
                        "created_at": "2026-11-23T01:00:00Z",
                        "reality": { "age_days": 0 },
                        "description": "Earlier today."
                    },
                    {
                        "public_slug": "senior-security-engineer-acme-1a2b",
                        "title": "Senior Security Engineer",
                        "company": "Acme Corp",
                        "url": "https://jobs.acme.example/sec-eng",
                        "location": "Berlin, Germany",
                        "work_mode": "remote",
                        "skills": ["rust", "aws", "appsec"],
                        "created_at": "2026-11-23T04:30:00Z",
                        "description": "## About\nProtect cloud infrastructure and APIs.",
                        "enrichment": {
                            "summary": "Lead product security initiatives across cloud and Rust services.",
                            "salary_min": 140000,
                            "salary_max": 180000,
                            "salary_currency": "USD",
                            "salary_period": "year",
                            "seniority": "senior",
                            "unknown_future_field": 12345
                        },
                        "reality": {
                            "age_days": 0
                        },
                        "future_top_level_field": "ignored-cleanly"
                    }
                ],
                "meta": {
                    "total": 25,
                    "limit": 10,
                    "offset": 10
                }
            }"###;

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            s2.write_all(resp.as_bytes()).await.expect("write 2");
        });

        let provider = FreehireProvider::with_retry_base_delay(
            format!("http://{addr}/api/v1"),
            Duration::from_millis(10),
        );
        let profiles = vec![SearchProfile::new(
            "p1",
            "Primary Role",
            "Security Engineer",
            vec!["Rust".to_string()],
        )];
        let filters = JobFilters {
            remote_only: true,
            country: Some("de".to_string()),
            posted_within_days: Some(1),
            manual_query: String::new(),
            page: 2,
            page_size: 10,
        };

        let result = provider
            .search(&profiles, &filters)
            .await
            .expect("retry should recover cleanly");
        server.await.expect("server task");

        // Stale 18d-old posting filtered out; remaining 3 strictly ordered newest-first!
        assert_eq!(result.jobs.len(), 3);
        assert_eq!(result.total_estimated, 25);
        assert_eq!(result.page, 2);
        assert!(result.has_next_page);

        // #1: Today at 04:30Z (Acme Corp)
        assert_eq!(result.jobs[0].company, "Acme Corp");
        assert_eq!(result.jobs[0].age_days, Some(0));
        assert_eq!(result.jobs[0].age_label.as_deref(), Some("Today"));
        assert_eq!(
            result.jobs[0].salary_display.as_deref(),
            Some("USD 140k-180k/yr")
        );
        assert_eq!(
            result.jobs[0].apply_url,
            "https://jobs.acme.example/sec-eng"
        );
        assert!(result.jobs[0].description_markdown.contains("## About"));

        // #2: Today at 01:00Z (BetaSec)
        assert_eq!(result.jobs[1].company, "BetaSec");
        assert_eq!(result.jobs[1].age_days, Some(0));

        // #3: 1d ago (DirectCo)
        assert_eq!(result.jobs[2].company, "DirectCo");
        assert_eq!(result.jobs[2].age_days, Some(1));
        assert_eq!(result.jobs[2].age_label.as_deref(), Some("1d ago"));
        assert_eq!(
            result.jobs[2].apply_url,
            "https://freehire.me/jobs/older-yesterday-role"
        );
        assert!(result.jobs[2].preview.contains("Own security reviews."));
    }

    #[tokio::test]
    async fn freehire_provider_recovers_from_truncated_stream_429_and_endpoint_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];

            // Attempt 1 on /agent/jobs/search: truncated JSON stream (Connection reset / short body)
            let (mut s1, _) = listener.accept().await.expect("accept 1");
            let _ = s1.read(&mut buf).await.expect("read 1");
            let truncated = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"data\":[{\"public_slug\":\"cut";
            s1.write_all(truncated.as_bytes()).await.expect("write 1");
            drop(s1);

            // Attempt 2 on /agent/jobs/search: HTTP 429 with Retry-After: 0
            let (mut s2, _) = listener.accept().await.expect("accept 2");
            let _ = s2.read(&mut buf).await.expect("read 2");
            let resp_429 = "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            s2.write_all(resp_429.as_bytes()).await.expect("write 2");
            drop(s2);

            // Attempt 3 on /agent/jobs/search: HTTP 502 Bad Gateway -> exhausts primary retries
            let (mut s3, _) = listener.accept().await.expect("accept 3");
            let _ = s3.read(&mut buf).await.expect("read 3");
            let resp_502 =
                "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            s3.write_all(resp_502.as_bytes()).await.expect("write 3");
            drop(s3);

            // Attempt 4: automatic fallback to standard `/jobs/search` succeeds!
            let (mut s4, _) = listener.accept().await.expect("accept 4");
            let n = s4.read(&mut buf).await.expect("read 4");
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            assert!(
                req.contains("GET /api/v1/jobs/search?"),
                "expected fallback to /jobs/search, got: {req}"
            );
            let ok_body = r#"{"data":[{"public_slug":"fallback-sec-1","title":"Security Engineer","company":"Fallback Labs","reality":{"age_days":0}}],"meta":{"total":1}}"#;
            let resp_ok = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                ok_body.len(),
                ok_body
            );
            s4.write_all(resp_ok.as_bytes()).await.expect("write 4");
        });

        let provider = FreehireProvider::with_retry_base_delay(
            format!("http://{addr}/api/v1"),
            Duration::from_millis(5),
        );
        let profiles = vec![SearchProfile::new(
            "p1",
            "Primary Role",
            "Security Engineer",
            vec!["Rust".to_string()],
        )];
        let filters = JobFilters::default();

        let result = provider
            .search(&profiles, &filters)
            .await
            .expect("multi-retry + /jobs/search fallback should recover automatically");
        server.await.expect("server task");

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].company, "Fallback Labs");
        assert!(
            !result.warnings.is_empty(),
            "fallback endpoint usage should attach an informational warning"
        );
    }

    #[tokio::test]
    #[ignore = "live network check against freehire.me"]
    async fn freehire_live_search_recovers_via_doh_alternate_edge() {
        let provider = FreehireProvider::default();
        let profiles = vec![SearchProfile::new(
            "p1",
            "Primary Role",
            "Penetration Tester",
            vec!["Security".to_string()],
        )];
        let filters = JobFilters {
            remote_only: false,
            country: None,
            posted_within_days: Some(30),
            manual_query: String::new(),
            page: 1,
            page_size: 10,
        };

        let t1 = Instant::now();
        let first = provider
            .search(&profiles, &filters)
            .await
            .expect("first live search should succeed via DoH failover if needed");
        let elapsed_first = t1.elapsed();

        let t2 = Instant::now();
        let second = provider
            .search(&profiles, &filters)
            .await
            .expect("second live search should reuse cached working edge");
        let elapsed_second = t2.elapsed();

        eprintln!(
            "LIVE_CHECK first={:?} ({} jobs, total={}) second={:?} ({} jobs, total={})",
            elapsed_first,
            first.jobs.len(),
            first.total_estimated,
            elapsed_second,
            second.jobs.len(),
            second.total_estimated,
        );
        assert!(!first.jobs.is_empty());
        assert!(!second.jobs.is_empty());
    }
}
