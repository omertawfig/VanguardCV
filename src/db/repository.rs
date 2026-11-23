//! `SqliteRepository` implementing [`crate::contracts::CvRepository`].
//!
//! # Design decisions
//!
//! * A single `Mutex<Connection>` serialises all access.  SQLite in WAL mode
//!   allows concurrent readers, but `rusqlite::Connection` itself is not
//!   `Sync`, so we wrap it.  For a Phase-4 performance pass one could switch
//!   to a connection-pool crate, but for now a mutex is correct and simple.
//! * JSON (de)serialisation for `pii_header_json`, `body_json`, and
//!   `job_context_json` is done with `serde_json`.
//! * Timestamps are stored as ISO-8601 strings (RFC 3339 / UTC).
//! * UUIDs are stored as `TEXT` (canonical hyphenated form).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::contracts::{
    BulletItem, CvDocument, CvSection, CvSummary, DocumentId, ItemId, JobContext, PiiContactHeader,
    PromoteOutcome, SanitizedCvBody, SectionKind, SkillCategoryItem, SnapshotSummary,
    TimelineEntry,
};

use super::migrations::run_migrations;

// ---------------------------------------------------------------------------
// SqliteRepository
// ---------------------------------------------------------------------------

pub struct SqliteRepository {
    conn: Mutex<Connection>,
    db_path: Option<PathBuf>,
}

impl SqliteRepository {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// Opens the database at `path`, creating it and its parent directories if
    /// needed.  WAL mode and foreign-key enforcement are enabled on every open.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating database directory {:?}", parent))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening SQLite database at {:?}", path))?;
        Self::configure_and_migrate(conn, Some(path.to_path_buf()))
    }

    /// Opens the database in the platform-appropriate user data directory:
    /// `{data_dir}/vanguard-cv/vanguard.db`.  Creates parent directories as
    /// needed.
    pub fn open_default() -> Result<Self> {
        let data_dir =
            dirs::data_dir().ok_or_else(|| anyhow!("cannot determine user data directory"))?;
        let db_path = data_dir.join("vanguard-cv").join("vanguard.db");
        Self::open(&db_path)
    }

    /// Opens an in-memory SQLite database (useful for unit tests).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure_and_migrate(conn, None)
    }

    fn configure_and_migrate(conn: Connection, db_path: Option<PathBuf>) -> Result<Self> {
        // WAL mode improves concurrent read throughput and robustness.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        f(&conn)
    }

    /// Seed only when the database contains no Root CVs.  Returns `true` if
    /// seeding was performed.
    pub fn seed_if_empty(&self) -> Result<bool> {
        let count: i64 = self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM cv_documents WHERE parent_id IS NULL",
                [],
                |row| row.get(0),
            )?)
        })?;

        if count > 0 {
            return Ok(false);
        }

        super::seed::seed(self)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Row ↔ domain helpers
// ---------------------------------------------------------------------------

fn parse_dt(s: &str) -> Result<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .with_context(|| format!("parsing datetime string {:?}", s))
}

/// Raw columns selected by `list_roots` and `list_children`.
type ListRow = (
    String,
    Option<String>,
    String,
    i64,
    String,
    String,
    Option<String>,
);

struct DocRow {
    id: String,
    parent_id: Option<String>,
    parent_version_snapshot: i64,
    title: String,
    version: i64,
    created_at: String,
    updated_at: String,
    job_context_json: Option<String>,
    pii_header_json: String,
    body_json: String,
}

fn row_to_doc(row: &DocRow) -> Result<CvDocument> {
    let id: DocumentId = Uuid::parse_str(&row.id)?;
    let parent_id: Option<DocumentId> =
        row.parent_id.as_deref().map(Uuid::parse_str).transpose()?;
    let pii_header: PiiContactHeader = serde_json::from_str(&row.pii_header_json)
        .with_context(|| format!("deserializing pii_header for {}", row.id))?;
    let body: SanitizedCvBody = serde_json::from_str(&row.body_json)
        .with_context(|| format!("deserializing body for {}", row.id))?;
    let job_context: Option<JobContext> = row
        .job_context_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .with_context(|| format!("deserializing job_context for {}", row.id))?;

    Ok(CvDocument {
        id,
        parent_id,
        parent_version_snapshot: row.parent_version_snapshot as u32,
        title: row.title.clone(),
        version: row.version as u32,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        job_context,
        pii_header,
        body,
    })
}

fn read_doc_row(conn: &Connection, id: DocumentId) -> Result<CvDocument> {
    let row = conn
        .query_row(
            "SELECT id, parent_id, parent_version_snapshot, title, version, \
             created_at, updated_at, job_context_json, pii_header_json, body_json \
             FROM cv_documents WHERE id = ?1",
            params![id.to_string()],
            |r| {
                Ok(DocRow {
                    id: r.get(0)?,
                    parent_id: r.get(1)?,
                    parent_version_snapshot: r.get(2)?,
                    title: r.get(3)?,
                    version: r.get(4)?,
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                    job_context_json: r.get(7)?,
                    pii_header_json: r.get(8)?,
                    body_json: r.get(9)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| anyhow!("CV document {} not found", id))?;

    row_to_doc(&row)
}

fn insert_doc(conn: &Connection, doc: &CvDocument) -> Result<()> {
    let pii_json = serde_json::to_string(&doc.pii_header)?;
    let body_json = serde_json::to_string(&doc.body)?;
    let job_json = doc
        .job_context
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    conn.execute(
        "INSERT INTO cv_documents \
         (id, parent_id, parent_version_snapshot, title, version, \
          created_at, updated_at, job_context_json, pii_header_json, body_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            doc.id.to_string(),
            doc.parent_id.map(|p| p.to_string()),
            doc.parent_version_snapshot as i64,
            doc.title,
            doc.version as i64,
            doc.created_at.to_rfc3339(),
            doc.updated_at.to_rfc3339(),
            job_json,
            pii_json,
            body_json,
        ],
    )?;
    Ok(())
}

fn update_doc(conn: &Connection, doc: &CvDocument) -> Result<()> {
    let pii_json = serde_json::to_string(&doc.pii_header)?;
    let body_json = serde_json::to_string(&doc.body)?;
    let job_json = doc
        .job_context
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let n = conn.execute(
        "UPDATE cv_documents SET \
         parent_version_snapshot = ?2, title = ?3, version = ?4, \
         updated_at = ?5, job_context_json = ?6, \
         pii_header_json = ?7, body_json = ?8 \
         WHERE id = ?1",
        params![
            doc.id.to_string(),
            doc.parent_version_snapshot as i64,
            doc.title,
            doc.version as i64,
            doc.updated_at.to_rfc3339(),
            job_json,
            pii_json,
            body_json,
        ],
    )?;

    if n == 0 {
        bail!("CV document {} not found", doc.id);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Deep clone body (Root → child variation)
// ---------------------------------------------------------------------------

fn deep_clone_body(body: &SanitizedCvBody) -> SanitizedCvBody {
    let sections = body
        .sections
        .iter()
        .map(|section| {
            let old_section_id = section.id;
            let kind = match &section.kind {
                SectionKind::Summary { text } => SectionKind::Summary { text: text.clone() },
                SectionKind::TimelineEntries { entries } => SectionKind::TimelineEntries {
                    entries: entries
                        .iter()
                        .map(|entry| {
                            let old_entry_id = entry.id;
                            TimelineEntry {
                                id: Uuid::new_v4(),
                                origin_item_id: Some(old_entry_id),
                                enabled: entry.enabled,
                                primary_heading: entry.primary_heading.clone(),
                                secondary_heading: entry.secondary_heading.clone(),
                                start_date: entry.start_date.clone(),
                                end_date: entry.end_date.clone(),
                                is_current: entry.is_current,
                                bullets: entry
                                    .bullets
                                    .iter()
                                    .map(|b| BulletItem {
                                        id: Uuid::new_v4(),
                                        origin_item_id: Some(b.id),
                                        enabled: b.enabled,
                                        text: b.text.clone(),
                                    })
                                    .collect(),
                                tags: entry.tags.clone(),
                            }
                        })
                        .collect(),
                },
                SectionKind::CategorizedInlineList { categories } => {
                    SectionKind::CategorizedInlineList {
                        categories: categories
                            .iter()
                            .map(|cat| SkillCategoryItem {
                                id: Uuid::new_v4(),
                                origin_item_id: Some(cat.id),
                                enabled: cat.enabled,
                                category_label: cat.category_label.clone(),
                                separator: cat.separator.clone(),
                                items: cat.items.clone(),
                            })
                            .collect(),
                    }
                }
                SectionKind::SimpleBulletList { items } => SectionKind::SimpleBulletList {
                    items: items
                        .iter()
                        .map(|b| BulletItem {
                            id: Uuid::new_v4(),
                            origin_item_id: Some(b.id),
                            enabled: b.enabled,
                            text: b.text.clone(),
                        })
                        .collect(),
                },
            };
            CvSection {
                id: Uuid::new_v4(),
                origin_item_id: Some(old_section_id),
                title: section.title.clone(),
                enabled: section.enabled,
                kind,
            }
        })
        .collect();
    SanitizedCvBody { sections }
}

// ---------------------------------------------------------------------------
// Summary query helpers
// ---------------------------------------------------------------------------

fn list_summaries(conn: &Connection, filter: Option<Option<&str>>) -> Result<Vec<CvSummary>> {
    // filter = None → all; Some(None) → roots only; Some(Some(id)) → children of id
    let (sql, root_id_param): (&str, Option<String>) = match filter {
        None => (
            "SELECT id, parent_id, title, version, created_at, updated_at, job_context_json \
             FROM cv_documents ORDER BY created_at",
            None,
        ),
        Some(None) => (
            "SELECT id, parent_id, title, version, created_at, updated_at, job_context_json \
             FROM cv_documents WHERE parent_id IS NULL ORDER BY created_at",
            None,
        ),
        Some(Some(id)) => (
            "SELECT id, parent_id, title, version, created_at, updated_at, job_context_json \
             FROM cv_documents WHERE parent_id = ?1 ORDER BY created_at",
            Some(id.to_string()),
        ),
    };

    let mut stmt = conn.prepare(sql)?;
    let rows_fn = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ListRow> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    };

    let raw: Vec<_> = match &root_id_param {
        None => stmt
            .query_map([], rows_fn)?
            .collect::<rusqlite::Result<_>>()?,
        Some(p) => stmt
            .query_map(params![p], rows_fn)?
            .collect::<rusqlite::Result<_>>()?,
    };

    let mut summaries = Vec::with_capacity(raw.len());
    for (id_str, parent_str, title, version, created_at, updated_at, job_json) in raw {
        let id: DocumentId = Uuid::parse_str(&id_str)?;
        let parent_id: Option<DocumentId> =
            parent_str.as_deref().map(Uuid::parse_str).transpose()?;
        let job_context: Option<JobContext> =
            job_json.as_deref().map(serde_json::from_str).transpose()?;

        // Count children (only relevant for roots, cheap for child rows where count = 0)
        let child_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cv_documents WHERE parent_id = ?1",
            params![id_str],
            |r| r.get(0),
        )?;

        summaries.push(CvSummary {
            id,
            parent_id,
            title,
            version: version as u32,
            created_at: parse_dt(&created_at)?,
            updated_at: parse_dt(&updated_at)?,
            job_context,
            child_count: child_count as usize,
        });
    }
    Ok(summaries)
}

/// Updates the `origin_item_id` of an item inside a child document after
/// promoting a brand-new item to its Root CV, preventing duplicates on repeat
/// promotions.
fn set_child_origin_item_id(doc: &mut CvDocument, item_id: ItemId, parent_item_id: ItemId) {
    for section in &mut doc.body.sections {
        if section.id == item_id {
            section.origin_item_id = Some(parent_item_id);
            return;
        }
        match &mut section.kind {
            SectionKind::TimelineEntries { entries } => {
                for entry in entries {
                    if entry.id == item_id {
                        entry.origin_item_id = Some(parent_item_id);
                        return;
                    }
                    for bullet in &mut entry.bullets {
                        if bullet.id == item_id {
                            bullet.origin_item_id = Some(parent_item_id);
                            return;
                        }
                    }
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for bullet in items {
                    if bullet.id == item_id {
                        bullet.origin_item_id = Some(parent_item_id);
                        return;
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for cat in categories {
                    if cat.id == item_id {
                        cat.origin_item_id = Some(parent_item_id);
                        return;
                    }
                }
            }
            SectionKind::Summary { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// CvRepository implementation
// ---------------------------------------------------------------------------

impl crate::contracts::CvRepository for SqliteRepository {
    fn list_roots(&self) -> Result<Vec<CvSummary>> {
        self.with_conn(|conn| list_summaries(conn, Some(None)))
    }

    fn list_children(&self, root_id: DocumentId) -> Result<Vec<CvSummary>> {
        self.with_conn(|conn| list_summaries(conn, Some(Some(&root_id.to_string()))))
    }

    fn list_all(&self) -> Result<Vec<CvSummary>> {
        self.with_conn(|conn| list_summaries(conn, None))
    }

    fn load(&self, id: DocumentId) -> Result<CvDocument> {
        self.with_conn(|conn| read_doc_row(conn, id))
    }

    fn create_root(
        &self,
        title: &str,
        pii: PiiContactHeader,
        body: SanitizedCvBody,
    ) -> Result<CvDocument> {
        let now = Utc::now();
        let doc = CvDocument {
            id: Uuid::new_v4(),
            parent_id: None,
            parent_version_snapshot: 1,
            title: title.to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            job_context: None,
            pii_header: pii,
            body,
        };
        self.with_conn(|conn| {
            insert_doc(conn, &doc)?;
            Ok(doc)
        })
    }

    fn save(&self, doc: &CvDocument) -> Result<()> {
        self.with_conn(|conn| {
            // Load the existing row to preserve created_at.
            let existing = read_doc_row(conn, doc.id)?;
            let mut next = doc.clone();
            next.version = existing.version + 1;
            next.updated_at = Utc::now();
            next.created_at = existing.created_at; // never overwrite
            update_doc(conn, &next)
        })
    }

    fn delete(&self, id: DocumentId) -> Result<()> {
        self.with_conn(|conn| {
            let child_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cv_documents WHERE parent_id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )?;
            if child_count > 0 {
                bail!(
                    "cannot delete Root CV {}: it still has {} child variation(s)",
                    id,
                    child_count
                );
            }
            let n = conn.execute(
                "DELETE FROM cv_documents WHERE id = ?1",
                params![id.to_string()],
            )?;
            if n == 0 {
                bail!("CV document {} not found", id);
            }
            Ok(())
        })
    }

    fn branch_sub_variation(
        &self,
        root_id: DocumentId,
        title: &str,
        job_ctx: JobContext,
    ) -> Result<CvDocument> {
        self.with_conn(|conn| {
            let root = read_doc_row(conn, root_id)?;
            if root.parent_id.is_some() {
                bail!("cannot branch a Child Sub-Variation; must branch from a Root CV");
            }

            let now = Utc::now();
            let child = CvDocument {
                id: Uuid::new_v4(),
                parent_id: Some(root_id),
                parent_version_snapshot: root.version,
                title: title.to_string(),
                version: 1,
                created_at: now,
                updated_at: now,
                job_context: Some(job_ctx),
                pii_header: root.pii_header.clone(),
                body: deep_clone_body(&root.body),
            };

            insert_doc(conn, &child)?;
            Ok(child)
        })
    }

    fn save_ai_batch(&self, doc: &CvDocument) -> Result<()> {
        self.with_conn(|conn| {
            // Enforce write-guard against the persisted database row, not just
            // the caller-supplied struct, so a forged `parent_id` cannot bypass it.
            let existing = read_doc_row(conn, doc.id)?;
            if existing.parent_id.is_none() || doc.parent_id.is_none() {
                bail!(
                    "write-guard: AI batch updates are not allowed on a Root CV (id={}). \
                     Branch a Child Sub-Variation first.",
                    doc.id
                );
            }
            let mut next = doc.clone();
            next.parent_id = existing.parent_id;
            next.version = existing.version + 1;
            next.updated_at = Utc::now();
            next.created_at = existing.created_at;
            update_doc(conn, &next)
        })
    }

    fn promote_item(&self, child_id: DocumentId, item_id: ItemId) -> Result<PromoteOutcome> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut child = read_doc_row(&tx, child_id)?;
            let root_id = child
                .parent_id
                .ok_or_else(|| anyhow!("promote_item requires a Child Sub-Variation"))?;
            let mut root = read_doc_row(&tx, root_id)?;

            // Locate the item in the child — summary section, timeline entry, bullet, or skill category
            enum FoundItem {
                SummarySection {
                    text: String,
                    section_origin_id: Option<ItemId>,
                    section_title: String,
                },
                TimelineBullet {
                    text: String,
                    origin_item_id: Option<ItemId>,
                    parent_entry_origin_id: Option<ItemId>,
                    parent_entry_heading: String,
                    section_origin_id: Option<ItemId>,
                    section_title: String,
                },
                SimpleBullet {
                    text: String,
                    origin_item_id: Option<ItemId>,
                    section_origin_id: Option<ItemId>,
                    section_title: String,
                },
                TimelineEntryItem {
                    entry: TimelineEntry,
                    section_origin_id: Option<ItemId>,
                    section_title: String,
                },
                SkillCategory {
                    label: String,
                    separator: String,
                    items: Vec<String>,
                    origin_item_id: Option<ItemId>,
                    section_origin_id: Option<ItemId>,
                    section_title: String,
                },
            }

            let found = 'find: {
                for section in &child.body.sections {
                    match &section.kind {
                        SectionKind::TimelineEntries { entries } => {
                            for entry in entries {
                                if entry.id == item_id {
                                    break 'find Some(FoundItem::TimelineEntryItem {
                                        entry: entry.clone(),
                                        section_origin_id: section.origin_item_id,
                                        section_title: section.title.clone(),
                                    });
                                }
                                for bullet in &entry.bullets {
                                    if bullet.id == item_id {
                                        break 'find Some(FoundItem::TimelineBullet {
                                            text: bullet.text.clone(),
                                            origin_item_id: bullet.origin_item_id,
                                            parent_entry_origin_id: entry.origin_item_id,
                                            parent_entry_heading: entry.primary_heading.clone(),
                                            section_origin_id: section.origin_item_id,
                                            section_title: section.title.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        SectionKind::SimpleBulletList { items } => {
                            for bullet in items {
                                if bullet.id == item_id {
                                    break 'find Some(FoundItem::SimpleBullet {
                                        text: bullet.text.clone(),
                                        origin_item_id: bullet.origin_item_id,
                                        section_origin_id: section.origin_item_id,
                                        section_title: section.title.clone(),
                                    });
                                }
                            }
                        }
                        SectionKind::CategorizedInlineList { categories } => {
                            for cat in categories {
                                if cat.id == item_id {
                                    break 'find Some(FoundItem::SkillCategory {
                                        label: cat.category_label.clone(),
                                        separator: cat.separator.clone(),
                                        items: cat.items.clone(),
                                        origin_item_id: cat.origin_item_id,
                                        section_origin_id: section.origin_item_id,
                                        section_title: section.title.clone(),
                                    });
                                }
                            }
                        }
                        SectionKind::Summary { text } => {
                            if section.id == item_id {
                                break 'find Some(FoundItem::SummarySection {
                                    text: text.clone(),
                                    section_origin_id: section.origin_item_id,
                                    section_title: section.title.clone(),
                                });
                            }
                        }
                    }
                }
                None
            };

            let found = found
                .ok_or_else(|| anyhow!("item {} not found in child CV {}", item_id, child_id))?;

            let mut link_child_origin = false;
            let mut bullet_origin_links: Vec<(ItemId, ItemId)> = Vec::new();
            let parent_item_id: ItemId = match found {
                FoundItem::SummarySection {
                    text,
                    section_origin_id,
                    section_title,
                } => {
                    let mut updated_id = None;
                    if let Some(origin) = section_origin_id
                        && let Some(root_sec) = root.body.sections.iter_mut().find(|s| s.id == origin)
                        && let SectionKind::Summary { text: root_text } = &mut root_sec.kind
                    {
                        *root_text = text.clone();
                        updated_id = Some(origin);
                    }
                    if updated_id.is_none() {
                        for root_sec in root.body.sections.iter_mut() {
                            if root_sec.title.eq_ignore_ascii_case(&section_title)
                                && let SectionKind::Summary { text: root_text } = &mut root_sec.kind
                            {
                                *root_text = text.clone();
                                updated_id = Some(root_sec.id);
                                link_child_origin = true;
                                break;
                            }
                        }
                    }
                    if let Some(id) = updated_id {
                        id
                    } else {
                        let new_id = Uuid::new_v4();
                        root.body.sections.insert(
                            0,
                            CvSection {
                                id: new_id,
                                origin_item_id: None,
                                title: section_title,
                                enabled: true,
                                kind: SectionKind::Summary { text },
                            },
                        );
                        link_child_origin = true;
                        new_id
                    }
                }
                FoundItem::TimelineBullet {
                    text,
                    origin_item_id,
                    parent_entry_origin_id,
                    parent_entry_heading,
                    section_origin_id,
                    section_title,
                } => {
                    // 1. Update matching root bullet by origin_item_id
                    let mut updated_origin = None;
                    if let Some(origin) = origin_item_id {
                        'outer: for section in root.body.sections.iter_mut() {
                            match &mut section.kind {
                                SectionKind::TimelineEntries { entries } => {
                                    for entry in entries.iter_mut() {
                                        for bullet in entry.bullets.iter_mut() {
                                            if bullet.id == origin {
                                                bullet.text = text.clone();
                                                updated_origin = Some(origin);
                                                break 'outer;
                                            }
                                        }
                                    }
                                }
                                SectionKind::SimpleBulletList { items } => {
                                    for bullet in items.iter_mut() {
                                        if bullet.id == origin {
                                            bullet.text = text.clone();
                                            updated_origin = Some(origin);
                                            break 'outer;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    if let Some(origin) = updated_origin {
                        origin
                    } else {
                        // 2. Append into the matching TimelineEntry in root (by origin or heading)
                        let new_bullet = BulletItem {
                            id: Uuid::new_v4(),
                            origin_item_id: None,
                            enabled: true,
                            text: text.clone(),
                        };
                        let new_id = new_bullet.id;
                        let mut appended = false;

                        'entry_match: for section in root.body.sections.iter_mut() {
                            if let SectionKind::TimelineEntries { entries } = &mut section.kind {
                                if let Some(entry_origin) = parent_entry_origin_id
                                    && let Some(target_entry) =
                                        entries.iter_mut().find(|e| e.id == entry_origin)
                                {
                                    target_entry.bullets.push(new_bullet.clone());
                                    appended = true;
                                    break 'entry_match;
                                }
                                if let Some(target_entry) = entries.iter_mut().find(|e| {
                                    e.primary_heading
                                        .eq_ignore_ascii_case(&parent_entry_heading)
                                }) {
                                    target_entry.bullets.push(new_bullet.clone());
                                    appended = true;
                                    break 'entry_match;
                                }
                            }
                        }

                        // 3. Fallback: append into matching timeline section's first entry or create SimpleBulletList
                        if !appended {
                            for section in root.body.sections.iter_mut() {
                                let matches_sec = section_origin_id == Some(section.id)
                                    || section.title.eq_ignore_ascii_case(&section_title);
                                if matches_sec
                                    && let SectionKind::TimelineEntries { entries } =
                                        &mut section.kind
                                    && let Some(first_entry) = entries.first_mut()
                                {
                                    first_entry.bullets.push(new_bullet.clone());
                                    appended = true;
                                    break;
                                }
                            }
                        }
                        if !appended {
                            root.body.sections.push(CvSection {
                                id: Uuid::new_v4(),
                                origin_item_id: None,
                                title: "ADDITIONAL ITEMS".to_string(),
                                enabled: true,
                                kind: SectionKind::SimpleBulletList {
                                    items: vec![new_bullet],
                                },
                            });
                        }

                        link_child_origin = true;
                        new_id
                    }
                }

                FoundItem::SimpleBullet {
                    text,
                    origin_item_id,
                    section_origin_id,
                    section_title,
                } => {
                    let mut updated_origin = None;
                    if let Some(origin) = origin_item_id {
                        'outer: for section in root.body.sections.iter_mut() {
                            match &mut section.kind {
                                SectionKind::SimpleBulletList { items } => {
                                    for bullet in items.iter_mut() {
                                        if bullet.id == origin {
                                            bullet.text = text.clone();
                                            updated_origin = Some(origin);
                                            break 'outer;
                                        }
                                    }
                                }
                                SectionKind::TimelineEntries { entries } => {
                                    for entry in entries.iter_mut() {
                                        for bullet in entry.bullets.iter_mut() {
                                            if bullet.id == origin {
                                                bullet.text = text.clone();
                                                updated_origin = Some(origin);
                                                break 'outer;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    if let Some(origin) = updated_origin {
                        origin
                    } else {
                        let new_bullet = BulletItem {
                            id: Uuid::new_v4(),
                            origin_item_id: None,
                            enabled: true,
                            text,
                        };
                        let new_id = new_bullet.id;
                        let mut appended = false;

                        // Prefer matching section by origin_item_id or title
                        for section in root.body.sections.iter_mut() {
                            let matches_sec = section_origin_id == Some(section.id)
                                || section.title.eq_ignore_ascii_case(&section_title);
                            if matches_sec
                                && let SectionKind::SimpleBulletList { items } = &mut section.kind
                            {
                                items.push(new_bullet.clone());
                                appended = true;
                                break;
                            }
                        }
                        if !appended {
                            for section in root.body.sections.iter_mut() {
                                if let SectionKind::SimpleBulletList { items } = &mut section.kind {
                                    items.push(new_bullet.clone());
                                    appended = true;
                                    break;
                                }
                            }
                        }
                        if !appended {
                            root.body.sections.push(CvSection {
                                id: Uuid::new_v4(),
                                origin_item_id: None,
                                title: section_title,
                                enabled: true,
                                kind: SectionKind::SimpleBulletList {
                                    items: vec![new_bullet],
                                },
                            });
                        }

                        link_child_origin = true;
                        new_id
                    }
                }

                FoundItem::TimelineEntryItem {
                    entry,
                    section_origin_id,
                    section_title,
                } => {
                    let mut updated_origin = None;
                    if let Some(origin) = entry.origin_item_id {
                        'outer: for section in root.body.sections.iter_mut() {
                            if let SectionKind::TimelineEntries { entries } = &mut section.kind
                                && let Some(root_entry) =
                                    entries.iter_mut().find(|e| e.id == origin)
                            {
                                root_entry.primary_heading = entry.primary_heading.clone();
                                root_entry.secondary_heading = entry.secondary_heading.clone();
                                root_entry.start_date = entry.start_date.clone();
                                root_entry.end_date = entry.end_date.clone();
                                root_entry.is_current = entry.is_current;
                                root_entry.tags = entry.tags.clone();
                                // Merge bullets: update matching root bullets in place and append
                                // new child bullets while preserving root-only bullets.
                                for b in &entry.bullets {
                                    if let Some(b_origin) = b.origin_item_id
                                        && let Some(root_b) =
                                            root_entry.bullets.iter_mut().find(|rb| rb.id == b_origin)
                                    {
                                        root_b.text = b.text.clone();
                                        root_b.enabled = b.enabled;
                                    } else {
                                        let new_bullet_id = Uuid::new_v4();
                                        root_entry.bullets.push(BulletItem {
                                            id: new_bullet_id,
                                            origin_item_id: None,
                                            enabled: b.enabled,
                                            text: b.text.clone(),
                                        });
                                        bullet_origin_links.push((b.id, new_bullet_id));
                                    }
                                }
                                updated_origin = Some(origin);
                                break 'outer;
                            }
                        }
                    }

                    if let Some(origin) = updated_origin {
                        origin
                    } else {
                        let promoted_bullets: Vec<BulletItem> = entry
                            .bullets
                            .into_iter()
                            .map(|b| {
                                let new_bullet_id = Uuid::new_v4();
                                bullet_origin_links.push((b.id, new_bullet_id));
                                BulletItem {
                                    id: new_bullet_id,
                                    origin_item_id: None,
                                    enabled: b.enabled,
                                    text: b.text,
                                }
                            })
                            .collect();
                        let new_entry = TimelineEntry {
                            id: Uuid::new_v4(),
                            origin_item_id: None,
                            enabled: true,
                            primary_heading: entry.primary_heading,
                            secondary_heading: entry.secondary_heading,
                            start_date: entry.start_date,
                            end_date: entry.end_date,
                            is_current: entry.is_current,
                            bullets: promoted_bullets,
                            tags: entry.tags,
                        };
                        let new_id = new_entry.id;
                        let mut appended = false;

                        for section in root.body.sections.iter_mut() {
                            let matches_sec = section_origin_id == Some(section.id)
                                || section.title.eq_ignore_ascii_case(&section_title);
                            if matches_sec
                                && let SectionKind::TimelineEntries { entries } = &mut section.kind
                            {
                                entries.push(new_entry.clone());
                                appended = true;
                                break;
                            }
                        }
                        if !appended {
                            for section in root.body.sections.iter_mut() {
                                if let SectionKind::TimelineEntries { entries } = &mut section.kind
                                {
                                    entries.push(new_entry.clone());
                                    appended = true;
                                    break;
                                }
                            }
                        }
                        if !appended {
                            root.body.sections.push(CvSection {
                                id: Uuid::new_v4(),
                                origin_item_id: None,
                                title: section_title,
                                enabled: true,
                                kind: SectionKind::TimelineEntries {
                                    entries: vec![new_entry],
                                },
                            });
                        }

                        link_child_origin = true;
                        new_id
                    }
                }

                FoundItem::SkillCategory {
                    label,
                    separator,
                    items,
                    origin_item_id,
                    section_origin_id,
                    section_title,
                } => {
                    let mut updated_origin = None;
                    if let Some(origin) = origin_item_id {
                        for section in root.body.sections.iter_mut() {
                            if let SectionKind::CategorizedInlineList { categories } =
                                &mut section.kind
                                && let Some(cat) = categories.iter_mut().find(|c| c.id == origin)
                            {
                                cat.category_label = label.clone();
                                cat.separator = separator.clone();
                                cat.items = items.clone();
                                updated_origin = Some(origin);
                                break;
                            }
                        }
                    }

                    if let Some(origin) = updated_origin {
                        origin
                    } else {
                        let new_cat = SkillCategoryItem {
                            id: Uuid::new_v4(),
                            origin_item_id: None,
                            enabled: true,
                            category_label: label,
                            separator,
                            items,
                        };
                        let new_id = new_cat.id;
                        let mut appended = false;

                        for section in root.body.sections.iter_mut() {
                            let matches_sec = section_origin_id == Some(section.id)
                                || section.title.eq_ignore_ascii_case(&section_title);
                            if matches_sec
                                && let SectionKind::CategorizedInlineList { categories } =
                                    &mut section.kind
                            {
                                categories.push(new_cat.clone());
                                appended = true;
                                break;
                            }
                        }
                        if !appended {
                            for section in root.body.sections.iter_mut() {
                                if let SectionKind::CategorizedInlineList { categories } =
                                    &mut section.kind
                                {
                                    categories.push(new_cat.clone());
                                    appended = true;
                                    break;
                                }
                            }
                        }
                        if !appended {
                            root.body.sections.push(CvSection {
                                id: Uuid::new_v4(),
                                origin_item_id: None,
                                title: section_title,
                                enabled: true,
                                kind: SectionKind::CategorizedInlineList {
                                    categories: vec![new_cat],
                                },
                            });
                        }

                        link_child_origin = true;
                        new_id
                    }
                }
            };

            root.version += 1;
            root.updated_at = Utc::now();
            update_doc(&tx, &root)?;

            if link_child_origin || !bullet_origin_links.is_empty() {
                if link_child_origin {
                    set_child_origin_item_id(&mut child, item_id, parent_item_id);
                }
                for (child_bullet_id, root_bullet_id) in bullet_origin_links {
                    set_child_origin_item_id(&mut child, child_bullet_id, root_bullet_id);
                }
                child.version += 1;
                child.updated_at = Utc::now();
                update_doc(&tx, &child)?;
            }

            tx.commit()?;
            Ok(PromoteOutcome {
                parent_item_id,
                parent_document_id: root_id,
            })
        })
    }

    fn create_snapshot(&self, cv_id: DocumentId, reason: &str) -> Result<SnapshotSummary> {
        self.with_conn(|conn| {
            let doc = read_doc_row(conn, cv_id)?;
            let snap_id = Uuid::new_v4();
            let created_at = Utc::now();
            let body_json = serde_json::to_string(&doc.body)?;

            conn.execute(
                "INSERT INTO cv_snapshots (id, cv_id, version, created_at, reason, body_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    snap_id.to_string(),
                    cv_id.to_string(),
                    doc.version as i64,
                    created_at.to_rfc3339(),
                    reason,
                    body_json,
                ],
            )?;

            Ok(SnapshotSummary {
                id: snap_id,
                cv_id,
                version: doc.version,
                created_at,
                reason: reason.to_string(),
            })
        })
    }

    fn list_snapshots(&self, cv_id: DocumentId) -> Result<Vec<SnapshotSummary>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, cv_id, version, created_at, reason \
                 FROM cv_snapshots WHERE cv_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt
                .query_map(params![cv_id.to_string()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            rows.into_iter()
                .map(|(id, cv_id_str, version, created_at, reason)| {
                    Ok(SnapshotSummary {
                        id: Uuid::parse_str(&id)?,
                        cv_id: Uuid::parse_str(&cv_id_str)?,
                        version: version as u32,
                        created_at: parse_dt(&created_at)?,
                        reason,
                    })
                })
                .collect()
        })
    }

    fn restore_snapshot(&self, snapshot_id: Uuid) -> Result<CvDocument> {
        self.with_conn(|conn| {
            // Load snapshot body
            let (cv_id_str, _snapshot_version, body_json): (String, i64, String) = conn
                .query_row(
                    "SELECT cv_id, version, body_json FROM cv_snapshots WHERE id = ?1",
                    params![snapshot_id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| anyhow!("snapshot {} not found", snapshot_id))?;

            let cv_id: DocumentId = Uuid::parse_str(&cv_id_str)?;
            let restored_body: SanitizedCvBody = serde_json::from_str(&body_json)?;
            let mut doc = read_doc_row(conn, cv_id)?;

            doc.body = restored_body;
            doc.version += 1; // strictly monotonic version bump
            doc.updated_at = Utc::now();
            update_doc(conn, &doc)?;
            Ok(doc)
        })
    }

    fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    fn load_setting(&self, key: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            let val = conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(val)
        })
    }

    fn export_json(&self, id: DocumentId) -> Result<String> {
        let doc = self.load(id)?;
        Ok(serde_json::to_string_pretty(&doc)?)
    }

    fn import_json(&self, json: &str) -> Result<CvDocument> {
        let mut doc: CvDocument =
            serde_json::from_str(json).context("parsing imported JSON as CvDocument")?;
        // Assign a fresh identity, regenerate all nested IDs, clear origin links,
        // and detach from any parent.
        let now = Utc::now();
        doc.id = Uuid::new_v4();
        doc.parent_id = None;
        doc.parent_version_snapshot = 1;
        doc.version = 1;
        doc.job_context = None;
        doc.created_at = now;
        doc.updated_at = now;
        for link in &mut doc.pii_header.social_links {
            link.id = Uuid::new_v4();
        }
        for field in &mut doc.pii_header.custom_fields {
            field.id = Uuid::new_v4();
        }
        for section in &mut doc.body.sections {
            section.id = Uuid::new_v4();
            section.origin_item_id = None;
            match &mut section.kind {
                SectionKind::Summary { .. } => {}
                SectionKind::TimelineEntries { entries } => {
                    for entry in entries {
                        entry.id = Uuid::new_v4();
                        entry.origin_item_id = None;
                        for bullet in &mut entry.bullets {
                            bullet.id = Uuid::new_v4();
                            bullet.origin_item_id = None;
                        }
                    }
                }
                SectionKind::CategorizedInlineList { categories } => {
                    for cat in categories {
                        cat.id = Uuid::new_v4();
                        cat.origin_item_id = None;
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    for bullet in items {
                        bullet.id = Uuid::new_v4();
                        bullet.origin_item_id = None;
                    }
                }
            }
        }

        self.with_conn(|conn| {
            insert_doc(conn, &doc)?;
            Ok(doc)
        })
    }

    fn backup_to(&self, path: &Path) -> Result<()> {
        if let Some(live_path) = &self.db_path {
            let same_file = match (std::fs::canonicalize(live_path), std::fs::canonicalize(path)) {
                (Ok(a), Ok(b)) => a == b,
                _ => live_path == path,
            };
            if same_file {
                bail!("cannot back up the database onto the currently open database file");
            }
        }

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating backup directory {:?}", parent))?;
        }

        // Use SQLite Online Backup API so backing up works for both file-backed
        // and in-memory databases and overwrites an existing target safely.
        self.with_conn(|conn| {
            let mut dst = Connection::open(path)
                .with_context(|| format!("opening backup destination {:?}", path))?;
            let backup = rusqlite::backup::Backup::new(conn, &mut dst)
                .context("initializing SQLite backup")?;
            backup
                .run_to_completion(100, Duration::from_millis(10), None)
                .context("writing SQLite backup")?;
            Ok(())
        })
    }

    fn reset_all(&self) -> Result<()> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("DELETE FROM cv_snapshots", [])?;
            tx.execute("DELETE FROM cv_documents WHERE parent_id IS NOT NULL", [])?;
            tx.execute("DELETE FROM cv_documents", [])?;
            tx.execute("DELETE FROM settings", [])?;
            // Keep workspace.seeded = true so the next app launch stays empty
            // instead of re-seeding the demo CV.
            tx.execute(
                "INSERT INTO settings (key, value) VALUES ('workspace.seeded', 'true')",
                [],
            )?;
            tx.commit()?;
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{BulletItem, CvRepository, JobContext, SectionKind};

    fn make_repo() -> SqliteRepository {
        SqliteRepository::in_memory().expect("in-memory DB")
    }

    fn sample_pii() -> PiiContactHeader {
        PiiContactHeader {
            full_name: "Jane Doe".to_string(),
            headline_job_title: Some("Software Engineer".to_string()),
            email: Some("jane@example.com".to_string()),
            phone: Some("+1 555 123 4567".to_string()),
            ..Default::default()
        }
    }

    fn sample_body() -> SanitizedCvBody {
        use crate::contracts::{CvSection, TimelineEntry};
        SanitizedCvBody {
            sections: vec![CvSection {
                id: Uuid::new_v4(),
                origin_item_id: None,
                title: "PROFESSIONAL EXPERIENCE".to_string(),
                enabled: true,
                kind: SectionKind::TimelineEntries {
                    entries: vec![{
                        let mut e = TimelineEntry::new("Acme Corp", "Jan 2022");
                        e.end_date = Some("Dec 2023".to_string());
                        e.bullets = vec![BulletItem::new("Did great things")];
                        e
                    }],
                },
            }],
        }
    }

    // --- create / load / save ---

    #[test]
    fn test_create_and_load() {
        let repo = make_repo();
        let doc = repo
            .create_root("Test CV", sample_pii(), sample_body())
            .unwrap();
        assert_eq!(doc.version, 1);
        assert!(doc.parent_id.is_none());

        let loaded = repo.load(doc.id).unwrap();
        assert_eq!(loaded.id, doc.id);
        assert_eq!(loaded.title, "Test CV");
        assert_eq!(loaded.pii_header.full_name, "Jane Doe");
        assert_eq!(loaded.body.sections.len(), 1);
    }

    #[test]
    fn test_save_bumps_version_preserves_created_at() {
        let repo = make_repo();
        let mut doc = repo
            .create_root("Version Test", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        let original_created_at = doc.created_at;
        doc.title = "Version Test Updated".to_string();
        repo.save(&doc).unwrap();

        let saved = repo.load(doc.id).unwrap();
        assert_eq!(saved.version, 2);
        assert_eq!(saved.created_at, original_created_at);
        assert_eq!(saved.title, "Version Test Updated");
    }

    #[test]
    fn test_list_roots_and_children() {
        let repo = make_repo();
        let root = repo
            .create_root("Root A", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        let _root2 = repo
            .create_root(
                "Root B",
                PiiContactHeader::default(),
                SanitizedCvBody::default(),
            )
            .unwrap();

        let roots = repo.list_roots().unwrap();
        assert_eq!(roots.len(), 2);

        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();
        let children = repo.list_children(root.id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child.id);
    }

    // --- branch clone ids + origin ids ---

    #[test]
    fn test_branch_gives_fresh_ids_and_origin_ids() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), sample_body())
            .unwrap();

        let root_section_id = root.body.sections[0].id;
        let root_bullet_id = match &root.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => entries[0].bullets[0].id,
            _ => panic!("wrong kind"),
        };

        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();

        // Child must have a different document id
        assert_ne!(child.id, root.id);
        assert_eq!(child.parent_id, Some(root.id));
        assert_eq!(child.parent_version_snapshot, root.version);

        // Section ids must differ, but origin_item_id must point back to root
        let child_section = &child.body.sections[0];
        assert_ne!(child_section.id, root_section_id);
        assert_eq!(child_section.origin_item_id, Some(root_section_id));

        // Bullet ids must differ, with origin
        let child_bullet = match &child_section.kind {
            SectionKind::TimelineEntries { entries } => &entries[0].bullets[0],
            _ => panic!("wrong kind"),
        };
        assert_ne!(child_bullet.id, root_bullet_id);
        assert_eq!(child_bullet.origin_item_id, Some(root_bullet_id));
    }

    // --- root write-guard ---

    #[test]
    fn test_root_write_guard() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        let err = repo.save_ai_batch(&root).unwrap_err();
        assert!(
            err.to_string().contains("write-guard"),
            "expected write-guard error, got: {err}"
        );

        // Even a forged parent_id on a persisted Root CV must be rejected
        let mut forged = root.clone();
        forged.parent_id = Some(Uuid::new_v4());
        let err = repo.save_ai_batch(&forged).unwrap_err();
        assert!(
            err.to_string().contains("write-guard"),
            "forged parent_id bypassed write-guard: {err}"
        );
    }

    // --- promote ---

    #[test]
    fn test_promote_bullet_with_origin() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), sample_body())
            .unwrap();
        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();

        // The child's bullet has an origin_item_id pointing at the root bullet.
        let child_bullet_id = match &child.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => entries[0].bullets[0].id,
            _ => panic!(),
        };
        let root_bullet_id = match &child.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => {
                entries[0].bullets[0].origin_item_id.unwrap()
            }
            _ => panic!(),
        };

        // Modify the child bullet text
        let mut updated_child = child.clone();
        if let SectionKind::TimelineEntries { entries } = &mut updated_child.body.sections[0].kind {
            entries[0].bullets[0].text = "New promoted text".to_string();
        }
        repo.save(&updated_child).unwrap();
        // Reload from DB
        let updated_child = repo.load(child.id).unwrap();

        let outcome = repo
            .promote_item(updated_child.id, child_bullet_id)
            .unwrap();
        assert_eq!(outcome.parent_item_id, root_bullet_id);
        assert_eq!(outcome.parent_document_id, root.id);

        // Root bullet should now have the new text
        let updated_root = repo.load(root.id).unwrap();
        let root_bullet_text = match &updated_root.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => entries[0].bullets[0].text.clone(),
            _ => panic!(),
        };
        assert_eq!(root_bullet_text, "New promoted text");
    }

    #[test]
    fn test_promote_new_timeline_bullet_lands_in_matching_entry_and_links_origin() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), sample_body())
            .unwrap();
        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();

        let new_bullet = BulletItem::new("Built zero-trust telemetry pipeline");
        let new_bullet_id = new_bullet.id;
        let mut updated_child = child.clone();
        if let SectionKind::TimelineEntries { entries } = &mut updated_child.body.sections[0].kind {
            entries[0].bullets.push(new_bullet);
        }
        repo.save(&updated_child).unwrap();

        let outcome = repo.promote_item(child.id, new_bullet_id).unwrap();
        let updated_root = repo.load(root.id).unwrap();
        match &updated_root.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => {
                assert_eq!(entries[0].bullets.len(), 2);
                assert_eq!(entries[0].bullets[1].id, outcome.parent_item_id);
                assert_eq!(
                    entries[0].bullets[1].text,
                    "Built zero-trust telemetry pipeline"
                );
            }
            _ => panic!("expected timeline section"),
        }

        // Promoting a second time after editing should update in place, not duplicate
        let mut child_reloaded = repo.load(child.id).unwrap();
        if let SectionKind::TimelineEntries { entries } = &mut child_reloaded.body.sections[0].kind
        {
            assert_eq!(
                entries[0].bullets[1].origin_item_id,
                Some(outcome.parent_item_id)
            );
            entries[0].bullets[1].text = "Built zero-trust telemetry pipeline v2".to_string();
        }
        repo.save(&child_reloaded).unwrap();
        let second_outcome = repo.promote_item(child.id, new_bullet_id).unwrap();
        assert_eq!(second_outcome.parent_item_id, outcome.parent_item_id);

        let root_after_second = repo.load(root.id).unwrap();
        match &root_after_second.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => {
                assert_eq!(entries[0].bullets.len(), 2);
                assert_eq!(
                    entries[0].bullets[1].text,
                    "Built zero-trust telemetry pipeline v2"
                );
            }
            _ => panic!("expected timeline section"),
        }
    }

    #[test]
    fn test_promote_bullet_no_origin_appends() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        // Add a SimpleBulletList to root
        let mut root_doc = repo.load(root.id).unwrap();
        let bullet = BulletItem::new("Root item");
        root_doc.body.sections.push(CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "HONORS AND AWARDS:".to_string(),
            enabled: true,
            kind: SectionKind::SimpleBulletList {
                items: vec![bullet.clone()],
            },
        });
        repo.save(&root_doc).unwrap();

        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();

        // Add a NEW bullet to child that has no origin_item_id
        let mut updated_child = child.clone();
        let new_bullet = BulletItem {
            id: Uuid::new_v4(),
            origin_item_id: None, // explicitly no origin
            enabled: true,
            text: "Brand new item".to_string(),
        };
        let new_bullet_id = new_bullet.id;
        match &mut updated_child.body.sections.last_mut().unwrap().kind {
            SectionKind::SimpleBulletList { items } => items.push(new_bullet),
            _ => panic!("expected simple bullet list"),
        }
        repo.save(&updated_child).unwrap();
        let updated_child = repo.load(child.id).unwrap();

        let outcome = repo.promote_item(updated_child.id, new_bullet_id).unwrap();
        assert_eq!(outcome.parent_document_id, root.id);

        let updated_root = repo.load(root.id).unwrap();
        // The root should contain the new item in a SimpleBulletList
        let found = updated_root.body.sections.iter().any(|s| match &s.kind {
            SectionKind::SimpleBulletList { items } => {
                items.iter().any(|b| b.text == "Brand new item")
            }
            _ => false,
        });
        assert!(found, "promoted bullet not found in root");
    }

    // --- snapshot restore ---

    #[test]
    fn test_snapshot_and_restore() {
        let repo = make_repo();
        let mut doc = repo
            .create_root("Snapshot Test", sample_pii(), sample_body())
            .unwrap();
        let snap = repo.create_snapshot(doc.id, "Pre-AI").unwrap();
        assert_eq!(snap.cv_id, doc.id);
        assert_eq!(snap.version, 1);

        // Mutate the doc multiple times so current version > snap.version + 1
        doc.body.sections[0].title = "CHANGED 1".to_string();
        repo.save(&doc).unwrap();
        doc.body.sections[0].title = "CHANGED 2".to_string();
        repo.save(&doc).unwrap();
        let before_restore = repo.load(doc.id).unwrap();
        assert_eq!(before_restore.version, 3);

        // Restore must advance monotonically past version 3 -> 4
        let restored = repo.restore_snapshot(snap.id).unwrap();
        assert_eq!(restored.body.sections[0].title, "PROFESSIONAL EXPERIENCE");
        assert_eq!(restored.version, 4);

        let snaps = repo.list_snapshots(doc.id).unwrap();
        assert!(!snaps.is_empty());
    }

    // --- import/export round-trip ---

    #[test]
    fn test_export_import_roundtrip() {
        let repo = make_repo();
        let root = repo
            .create_root("Export Test", sample_pii(), sample_body())
            .unwrap();
        let json = repo.export_json(root.id).unwrap();
        let imported = repo.import_json(&json).unwrap();

        // New id, detached parent
        assert_ne!(imported.id, root.id);
        assert!(imported.parent_id.is_none());
        assert!(imported.job_context.is_none());

        // Content preserved
        assert_eq!(imported.title, root.title);
        assert_eq!(imported.body.sections.len(), root.body.sections.len());
        assert_eq!(imported.pii_header.full_name, root.pii_header.full_name);
    }

    // --- settings ---

    #[test]
    fn test_save_load_setting() {
        let repo = make_repo();
        repo.save_setting("theme", "dark").unwrap();
        let val = repo.load_setting("theme").unwrap();
        assert_eq!(val.as_deref(), Some("dark"));
        // Overwrite
        repo.save_setting("theme", "light").unwrap();
        let val = repo.load_setting("theme").unwrap();
        assert_eq!(val.as_deref(), Some("light"));
        // Missing key
        assert!(repo.load_setting("missing").unwrap().is_none());
    }

    // --- delete ---

    #[test]
    fn test_delete_root_with_children_fails() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        let _child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();
        let err = repo.delete(root.id).unwrap_err();
        assert!(err.to_string().contains("child variation"));
    }

    #[test]
    fn test_delete_child_ok() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();
        repo.delete(child.id).unwrap();
        let err = repo.load(child.id).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // --- backup_to ---

    #[test]
    fn test_backup_to() {
        let repo = SqliteRepository::in_memory().unwrap();
        let root = repo
            .create_root("Backup Test", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        repo.backup_to(tmp.path()).unwrap();

        let restored = SqliteRepository::open(tmp.path()).unwrap();
        let loaded = restored.load(root.id).unwrap();
        assert_eq!(loaded.title, "Backup Test");
    }

    // --- promote skill category ---

    #[test]
    fn test_promote_skill_category() {
        let repo = make_repo();
        let root = repo
            .create_root("Root", sample_pii(), SanitizedCvBody::default())
            .unwrap();
        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();

        // Add a NEW skill category to child (no origin)
        let mut child_doc = child.clone();
        let new_cat = SkillCategoryItem {
            id: Uuid::new_v4(),
            origin_item_id: None,
            enabled: true,
            category_label: "Languages".to_string(),
            separator: " - ".to_string(),
            items: vec!["Rust".to_string(), "Python".to_string()],
        };
        let new_cat_id = new_cat.id;
        child_doc.body.sections.push(CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "SKILLS AND OTHERS".to_string(),
            enabled: true,
            kind: SectionKind::CategorizedInlineList {
                categories: vec![new_cat],
            },
        });
        repo.save(&child_doc).unwrap();
        let child_doc = repo.load(child.id).unwrap();

        let outcome = repo.promote_item(child_doc.id, new_cat_id).unwrap();
        assert_eq!(outcome.parent_document_id, root.id);

        let updated_root = repo.load(root.id).unwrap();
        let found = updated_root.body.sections.iter().any(|s| match &s.kind {
            SectionKind::CategorizedInlineList { categories } => {
                categories.iter().any(|c| c.category_label == "Languages")
            }
            _ => false,
        });
        assert!(found, "skill category not promoted to root");
    }

    #[test]
    fn test_import_json_rekeys_inner_ids_and_resets_version() {
        let repo = make_repo();
        let mut root = repo
            .create_root("Root", sample_pii(), sample_body())
            .unwrap();
        root.title = "Root v2".to_string();
        repo.save(&root).unwrap();
        let child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();

        let json = repo.export_json(child.id).unwrap();
        let imported = repo.import_json(&json).unwrap();
        assert_eq!(imported.version, 1);
        assert_ne!(imported.body.sections[0].id, child.body.sections[0].id);
        assert!(imported.body.sections[0].origin_item_id.is_none());
        if let SectionKind::TimelineEntries { entries } = &imported.body.sections[0].kind {
            assert!(entries[0].origin_item_id.is_none());
            assert!(entries[0].bullets[0].origin_item_id.is_none());
        } else {
            panic!("expected timeline section");
        }
    }

    #[test]
    fn test_backup_to_rejects_live_database_path() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("live.db");
        let repo = SqliteRepository::open(&db_path).unwrap();
        let err = repo.backup_to(&db_path).unwrap_err();
        assert!(err.to_string().contains("currently open database"));
    }

    #[test]
    fn test_promote_timeline_entry_preserves_root_bullets_and_links_new_bullets() {
        let repo = make_repo();
        let mut root = repo
            .create_root("Root", sample_pii(), sample_body())
            .unwrap();
        let root_only_bullet = BulletItem::new("Root-only archival bullet");
        if let SectionKind::TimelineEntries { entries } = &mut root.body.sections[0].kind {
            entries[0].bullets.push(root_only_bullet);
        }
        repo.save(&root).unwrap();

        let mut child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();
        let child_entry_id = match &mut child.body.sections[0].kind {
            SectionKind::TimelineEntries { entries } => {
                // Remove the root-only bullet in the child variation and add a new tailored bullet
                entries[0].bullets.pop();
                entries[0].bullets[0].text = "Updated tailored bullet".to_string();
                entries[0]
                    .bullets
                    .push(BulletItem::new("Brand new child bullet"));
                entries[0].id
            }
            _ => panic!("expected timeline section"),
        };
        repo.save(&child).unwrap();

        repo.promote_item(child.id, child_entry_id).unwrap();

        let updated_root = repo.load(root.id).unwrap();
        let updated_child = repo.load(child.id).unwrap();
        if let SectionKind::TimelineEntries { entries } = &updated_root.body.sections[0].kind {
            let texts: Vec<&str> = entries[0].bullets.iter().map(|b| b.text.as_str()).collect();
            assert!(texts.contains(&"Updated tailored bullet"));
            assert!(texts.contains(&"Root-only archival bullet"));
            assert!(texts.contains(&"Brand new child bullet"));
        } else {
            panic!("expected timeline section");
        }
        if let SectionKind::TimelineEntries { entries } = &updated_child.body.sections[0].kind {
            assert!(entries[0].bullets[1].origin_item_id.is_some());
        }
    }

    #[test]
    fn test_promote_summary_section_updates_root() {
        let repo = make_repo();
        let body = SanitizedCvBody {
            sections: vec![CvSection {
                id: Uuid::new_v4(),
                origin_item_id: None,
                title: "SUMMARY".to_string(),
                enabled: true,
                kind: SectionKind::Summary {
                    text: "Original summary".to_string(),
                },
            }],
        };
        let root = repo.create_root("Root", sample_pii(), body).unwrap();
        let mut child = repo
            .branch_sub_variation(root.id, "Child", JobContext::default())
            .unwrap();
        let child_sec_id = child.body.sections[0].id;
        child.body.sections[0].kind = SectionKind::Summary {
            text: "Tailored executive summary".to_string(),
        };
        repo.save(&child).unwrap();

        repo.promote_item(child.id, child_sec_id).unwrap();
        let updated_root = repo.load(root.id).unwrap();
        assert_eq!(
            updated_root.body.sections[0].kind,
            SectionKind::Summary {
                text: "Tailored executive summary".to_string()
            }
        );
    }

    #[test]
    fn test_reset_all_wipes_everything_and_keeps_seeded_flag() {
        let repo = make_repo();
        let root = repo.create_root("Master", sample_pii(), sample_body()).unwrap();
        let child = repo
            .branch_sub_variation(root.id, "Variation", JobContext::default())
            .unwrap();
        repo.create_snapshot(root.id, "checkpoint").unwrap();
        repo.save_setting("workspace.default_cv_id", &root.id.to_string()).unwrap();
        repo.save_setting("jobs.saved_profiles_v1", "{\"x\":1}").unwrap();

        repo.reset_all().expect("reset_all should succeed");

        assert!(repo.list_roots().unwrap().is_empty());
        assert!(repo.load(root.id).is_err());
        assert!(repo.load(child.id).is_err());
        assert!(repo.list_snapshots(root.id).unwrap().is_empty());
        assert_eq!(repo.load_setting("workspace.default_cv_id").unwrap(), None);
        assert_eq!(repo.load_setting("jobs.saved_profiles_v1").unwrap(), None);
        assert_eq!(
            repo.load_setting("workspace.seeded").unwrap().as_deref(),
            Some("true")
        );
    }
}
