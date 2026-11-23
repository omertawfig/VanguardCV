-- VanguardCV database schema v1
-- Applied idempotently via PRAGMA user_version check in src/db/migrations.rs

CREATE TABLE cv_documents (
    id                      TEXT PRIMARY KEY,
    parent_id               TEXT REFERENCES cv_documents(id) ON DELETE RESTRICT,
    parent_version_snapshot INTEGER NOT NULL DEFAULT 1,
    title                   TEXT NOT NULL,
    version                 INTEGER NOT NULL DEFAULT 1,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    job_context_json        TEXT,          -- NULL for Root CVs; JSON for children
    pii_header_json         TEXT NOT NULL,
    body_json               TEXT NOT NULL
);

CREATE INDEX idx_cv_parent ON cv_documents(parent_id);

CREATE TABLE cv_snapshots (
    id          TEXT PRIMARY KEY,
    cv_id       TEXT NOT NULL REFERENCES cv_documents(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    created_at  TEXT NOT NULL,
    reason      TEXT NOT NULL,
    body_json   TEXT NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
