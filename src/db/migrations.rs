//! SQLite migrations for `cv_documents`, `cv_snapshots`, and `settings`.
//!
//! Schema is versioned via `PRAGMA user_version`.  `run_migrations` is
//! idempotent: calling it on an already-up-to-date database is a no-op.

use anyhow::Result;
use rusqlite::Connection;

/// Current schema version.  Increment whenever a new migration is added.
const CURRENT_VERSION: u32 = 1;

/// Runs all pending migrations inside a transaction.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version >= CURRENT_VERSION {
        return Ok(());
    }

    let tx = conn.unchecked_transaction()?;

    // Migration 1 — initial schema
    if version < 1 {
        tx.execute_batch(include_str!("../../migrations/001_init.sql"))?;
    }

    tx.pragma_update(None, "user_version", CURRENT_VERSION)?;
    tx.commit()?;
    Ok(())
}
