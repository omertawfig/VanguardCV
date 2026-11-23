//! Realistic Root CV seed data.
//!
//! `seed(repo)` inserts one realistic Root CV (matching the reference photo
//! structure) when the database has no Root CVs.  Call via
//! [`super::repository::SqliteRepository::seed_if_empty`].

use anyhow::Result;
use uuid::Uuid;

use crate::contracts::{
    BulletItem, CvDocument, CvRepository, CvSection, CustomHeaderField, PiiContactHeader,
    SanitizedCvBody, SectionKind, SkillCategoryItem, SocialLink, TimelineEntry,
};

/// Insert a realistic Root CV seed document into `repo`.
///
/// Does **not** check whether the DB is empty; that guard lives in
/// `SqliteRepository::seed_if_empty`.
pub fn seed(repo: &impl CvRepository) -> Result<()> {
    let pii = PiiContactHeader {
        full_name: "Jordan Blake".to_string(),
        headline_job_title: Some("Offensive Security Engineer".to_string()),
        email: Some("jordan.blake@protonmail.com".to_string()),
        phone: Some("+1 (555) 847-2991".to_string()),
        location: Some("Remote — EU/US timezone".to_string()),
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
    };

    // -----------------------------------------------------------------------
    // PROFESSIONAL EXPERIENCE
    // -----------------------------------------------------------------------
    let experience_section = {
        let entry_bug_bounty = {
            let mut e = TimelineEntry::new("Independent Bug Bounty Research", "Mar 2021");
            e.is_current = true;
            e.secondary_heading = Some("Security Researcher".to_string());
            e.bullets = vec![
                BulletItem::new(
                    "Reported 60+ critical and high-severity vulnerabilities across HackerOne \
                     and Bugcrowd programs, earning cumulative bounties exceeding $45 000.",
                ),
                BulletItem::new(
                    "Built reproducible proof-of-concept exploits for SSRF, IDOR, and \
                     authentication-bypass chains in SaaS platforms serving millions of users.",
                ),
                BulletItem::new(
                    "Authored triage-ready remediation writeups that reduced average vendor \
                     fix time by approximately 40%.",
                ),
            ];
            e.tags = vec!["bug-bounty".to_string(), "web".to_string()];
            e
        };

        let entry_crowdstrike = {
            let mut e = TimelineEntry::new("CrowdStrike", "Apr 2020");
            e.secondary_heading = Some("Security Analyst — Detection Engineering".to_string());
            e.start_date = "Apr 2020".to_string();
            e.end_date = Some("Feb 2021".to_string());
            e.bullets = vec![
                BulletItem::new(
                    "Automated alert triage in Python, reducing mean time to respond by 30% \
                     across the EMEA falcon deployment.",
                ),
                BulletItem::new(
                    "Wrote 15+ custom detection rules in Splunk SPL to surface novel \
                     lateral-movement patterns identified during threat-hunting cycles.",
                ),
            ];
            e.tags = vec!["detection".to_string(), "splunk".to_string()];
            e
        };

        let entry_intern = {
            let mut e = TimelineEntry::new("SecureWave Labs", "Jun 2019");
            e.secondary_heading = Some("Penetration Testing Intern".to_string());
            e.end_date = Some("Mar 2020".to_string());
            e.bullets = vec![
                BulletItem::new(
                    "Conducted external network and web-application penetration tests for \
                     12 SME clients, delivering executive and technical reports.",
                ),
                BulletItem::new(
                    "Developed a Burp Suite extension in Python to automate XSS parameter \
                     fuzzing, cutting manual testing effort by 25%.",
                ),
            ];
            e
        };

        CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "PROFESSIONAL EXPERIENCE".to_string(),
            enabled: true,
            kind: SectionKind::TimelineEntries {
                entries: vec![entry_bug_bounty, entry_crowdstrike, entry_intern],
            },
        }
    };

    // -----------------------------------------------------------------------
    // EDUCATION
    // -----------------------------------------------------------------------
    let education_section = {
        let entry_uni = {
            let mut e = TimelineEntry::new("University of Applied Sciences — Darmstadt", "Sep 2015");
            e.secondary_heading = Some("B.Sc. Computer Science".to_string());
            e.end_date = Some("Jun 2019".to_string());
            e.bullets = vec![
                BulletItem::new(
                    "Specialisation: network security, distributed systems, and applied \
                     cryptography.",
                ),
                BulletItem::new(
                    "Final thesis: Automated Detection of Server-Side Request Forgery via \
                     Differential Analysis (grade: 1.3 / GPA 3.9 equivalent).",
                ),
            ];
            e
        };

        CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "EDUCATION".to_string(),
            enabled: true,
            kind: SectionKind::TimelineEntries {
                entries: vec![entry_uni],
            },
        }
    };

    // -----------------------------------------------------------------------
    // SKILLS AND OTHERS
    // -----------------------------------------------------------------------
    let skills_section = CvSection {
        id: Uuid::new_v4(),
        origin_item_id: None,
        title: "SKILLS AND OTHERS".to_string(),
        enabled: true,
        kind: SectionKind::CategorizedInlineList {
            categories: vec![
                SkillCategoryItem {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    enabled: true,
                    category_label: "Soft skills".to_string(),
                    separator: " - ".to_string(),
                    items: vec![
                        "Clear written & verbal communication".to_string(),
                        "Ownership & self-direction".to_string(),
                        "Mentoring junior researchers".to_string(),
                        "Cross-functional collaboration".to_string(),
                    ],
                },
                SkillCategoryItem {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    enabled: true,
                    category_label: "Technical skills".to_string(),
                    separator: " - ".to_string(),
                    items: vec![
                        "Rust (advanced)".to_string(),
                        "Python (advanced)".to_string(),
                        "Burp Suite Professional".to_string(),
                        "Metasploit / Cobalt Strike".to_string(),
                        "Splunk / SIEM tuning".to_string(),
                        "Docker / Kubernetes (working knowledge)".to_string(),
                    ],
                },
                SkillCategoryItem {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    enabled: true,
                    category_label: "Languages".to_string(),
                    separator: " - ".to_string(),
                    items: vec![
                        "English (native)".to_string(),
                        "German (B2)".to_string(),
                        "Spanish (A2)".to_string(),
                    ],
                },
            ],
        },
    };

    // -----------------------------------------------------------------------
    // HONORS AND AWARDS
    // -----------------------------------------------------------------------
    let honors_section = CvSection {
        id: Uuid::new_v4(),
        origin_item_id: None,
        title: "HONORS AND AWARDS:".to_string(),
        enabled: true,
        kind: SectionKind::SimpleBulletList {
            items: vec![
                BulletItem::new("Top 1% HackerOne signal ranking, 2024."),
                BulletItem::new(
                    "Bugcrowd MVP — Q3 2023 (ranked #3 globally by signal score).",
                ),
                BulletItem::new(
                    "Speaker — BSides Munich 2023: \"From SSRF to Cloud-Root: A Real-World \
                     Bug-Bounty Chain\".",
                ),
            ],
        },
    };

    // -----------------------------------------------------------------------
    // TRAINING COURSES
    // -----------------------------------------------------------------------
    let training_section = CvSection {
        id: Uuid::new_v4(),
        origin_item_id: None,
        title: "TRAINING COURSES:".to_string(),
        enabled: true,
        kind: SectionKind::SimpleBulletList {
            items: vec![
                BulletItem::new(
                    "Offensive Security Certified Professional (OSCP) — OffSec, 2022.",
                ),
                BulletItem::new(
                    "Web Application Penetration Tester eXtreme (eWPTX) — INE / eLearnSecurity, \
                     2023.",
                ),
                BulletItem::new(
                    "Certified Kubernetes Security Specialist (CKS) — Linux Foundation, 2024.",
                ),
            ],
        },
    };

    // -----------------------------------------------------------------------
    // Assemble body & persist
    // -----------------------------------------------------------------------
    let body = SanitizedCvBody {
        sections: vec![
            experience_section,
            education_section,
            skills_section,
            honors_section,
            training_section,
        ],
    };

    let mut doc: CvDocument =
        repo.create_root("Cybersecurity & Pentesting Master", pii, body)?;
    // Keep the initial version at 1 — no need to bump
    let _ = &mut doc;

    Ok(())
}
