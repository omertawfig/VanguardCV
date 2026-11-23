//! Local SQLite storage, lineage, snapshots, and sync operations.
//!
//! **Owner:** Sub-Agent 1 (Data, Lineage & Privacy Engineer).

pub mod archive;
pub mod lineage_diff;
pub mod migrations;
pub mod repository;
pub mod seed;
pub mod snapshots;
pub mod sync;
