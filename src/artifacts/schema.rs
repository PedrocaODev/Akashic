use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

use super::lineage::fingerprint_parts;
use super::{hash, Error};

pub(crate) const SCHEMA: i64 = 1;
pub(crate) const STORE_SCHEMA: i64 = 3;

#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::marker::PhantomData;
#[cfg(test)]
use std::rc::Rc;
#[cfg(test)]
thread_local! { static MIGRATION_FAILPOINT: Cell<bool> = const { Cell::new(false) }; }

pub(crate) type LegacyRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
);
pub(crate) type MigrationRow = (
    i64,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
);

#[derive(Clone, Copy)]
pub(crate) struct ValidationContext {
    pub(super) origin_version: i64,
    pub(super) target_version: i64,
    pub(super) metadata_finalized: bool,
}

impl ValidationContext {
    pub(super) fn expected_metadata(self) -> (i64, i64) {
        (
            if self.metadata_finalized {
                self.target_version
            } else {
                self.origin_version
            },
            1,
        )
    }
}

pub(super) fn create_schema(db: &Transaction<'_>) -> Result<(), Error> {
    db.execute_batch("CREATE TABLE schema_metadata(key TEXT PRIMARY KEY,value INTEGER NOT NULL); INSERT INTO schema_metadata VALUES('schema_version',1),('migration_lineage',1); CREATE TABLE artifacts(id TEXT PRIMARY KEY,owner TEXT NOT NULL,source TEXT NOT NULL); CREATE TABLE versions(id TEXT PRIMARY KEY,artifact_id TEXT NOT NULL,hash TEXT NOT NULL,canonical TEXT NOT NULL,ancestry TEXT,operation TEXT NOT NULL UNIQUE,actor TEXT NOT NULL,source TEXT NOT NULL,observed_identity TEXT,expected_hash TEXT,schema_version INTEGER NOT NULL,FOREIGN KEY(artifact_id) REFERENCES artifacts(id),FOREIGN KEY(ancestry) REFERENCES versions(id)); CREATE TABLE operations(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,result TEXT NOT NULL,version_id TEXT,lineage TEXT,rejection_code TEXT,schema_version INTEGER NOT NULL,FOREIGN KEY(version_id) REFERENCES versions(id)); CREATE TABLE lineage(operation_id TEXT PRIMARY KEY,version_id TEXT NOT NULL,parent_version_id TEXT,FOREIGN KEY(operation_id) REFERENCES operations(id),FOREIGN KEY(version_id) REFERENCES versions(id),FOREIGN KEY(parent_version_id) REFERENCES versions(id)); CREATE TABLE discrepancies(id INTEGER PRIMARY KEY,operation TEXT NOT NULL UNIQUE,actor TEXT NOT NULL,artifact_id TEXT NOT NULL,source TEXT NOT NULL,ancestry TEXT,reason TEXT NOT NULL,status TEXT NOT NULL,operation_result_id TEXT NOT NULL,expected_hash TEXT,observed_hash TEXT,context TEXT NOT NULL); CREATE INDEX versions_artifact ON versions(artifact_id); CREATE TRIGGER immutable_versions_update BEFORE UPDATE ON versions BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_versions_delete BEFORE DELETE ON versions BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_operations_update BEFORE UPDATE ON operations BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_operations_delete BEFORE DELETE ON operations BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_lineage_update BEFORE UPDATE ON lineage BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_lineage_delete BEFORE DELETE ON lineage BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_discrepancies_update BEFORE UPDATE ON discrepancies BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_discrepancies_delete BEFORE DELETE ON discrepancies BEGIN SELECT RAISE(ABORT,'immutable'); END;").map_err(Error::Sql)?;
    db.execute_batch("ALTER TABLE artifacts ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1; ALTER TABLE lineage ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1; ALTER TABLE versions ADD COLUMN source_schema_version INTEGER NOT NULL DEFAULT 1; ALTER TABLE versions ADD COLUMN source_lineage TEXT NOT NULL DEFAULT 'native'; ALTER TABLE discrepancies ADD COLUMN expected_identity TEXT; ALTER TABLE discrepancies ADD COLUMN observed_identity TEXT; ALTER TABLE discrepancies ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1; CREATE TABLE compatibility_decisions(id INTEGER PRIMARY KEY,source_schema_version INTEGER NOT NULL,target_schema_version INTEGER NOT NULL,operation TEXT NOT NULL,actor TEXT NOT NULL,source TEXT NOT NULL,outcome TEXT NOT NULL,fingerprint TEXT NOT NULL UNIQUE,result TEXT NOT NULL);").map_err(Error::Sql)?;
    db.execute_batch("ALTER TABLE discrepancies ADD COLUMN proposed_hash TEXT; ALTER TABLE discrepancies ADD COLUMN observed_owner TEXT; ALTER TABLE discrepancies ADD COLUMN observed_source TEXT; ALTER TABLE discrepancies ADD COLUMN observed_ancestry TEXT;")?;
    Ok(())
}

pub(super) fn ensure_record_schema(db: &Transaction<'_>) -> Result<(), Error> {
    // Version observation columns are v3-only; keeping them out of this helper
    // preserves the frozen v2 descriptor.
    ensure_runtime_record_schema(db)
}

pub(super) fn prepare_canonical_v2(db: &Transaction<'_>) -> Result<(), Error> {
    if table_exists(db, "compatibility_decisions")? {
        db.execute_batch("DROP TABLE compatibility_decisions;")?;
    }
    ensure_compatibility_decision_table(db)?;
    ensure_record_schema(db)
}

pub(super) fn install_version_observation_columns(db: &Transaction<'_>) -> Result<(), Error> {
    for (name, definition) in [
        ("logical_path", "BLOB"),
        ("git_applicability", "TEXT"),
        ("git_repository_path", "BLOB"),
        ("git_head_state", "TEXT"),
        ("git_head_oid", "TEXT"),
        ("git_index_present", "INTEGER"),
        ("git_index_fingerprint", "TEXT"),
    ] {
        if db.query_row(
            &format!("SELECT count(*) FROM pragma_table_info('versions') WHERE name='{name}'"),
            [],
            |r| r.get::<_, i64>(0),
        )? == 0
        {
            db.execute_batch(&format!(
                "ALTER TABLE versions ADD COLUMN {name} {definition}"
            ))?;
        }
    }
    Ok(())
}

pub(super) fn ensure_runtime_record_schema(db: &Transaction<'_>) -> Result<(), Error> {
    let had_events = table_exists(db, "events")?;
    db.execute_batch("\
CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL CHECK(schema_version=1),source_lineage TEXT NOT NULL,correction_of TEXT,event_sequence INTEGER NOT NULL DEFAULT 0,transition TEXT NOT NULL DEFAULT '',deterministic_input TEXT NOT NULL DEFAULT '',outcome TEXT);
CREATE TABLE IF NOT EXISTS findings(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL CHECK(schema_version=1),source_lineage TEXT NOT NULL,correction_of TEXT);
CREATE TABLE IF NOT EXISTS evidence(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),source_lineage TEXT NOT NULL,correction_of TEXT);
CREATE TABLE IF NOT EXISTS history(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),source_lineage TEXT NOT NULL,correction_of TEXT);
CREATE TABLE IF NOT EXISTS replay_metadata(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),source_lineage TEXT NOT NULL,correction_of TEXT);
CREATE TABLE IF NOT EXISTS lifecycle_evidence(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),status TEXT NOT NULL CHECK(status IN ('created','invalidated')),actor TEXT NOT NULL,reason TEXT NOT NULL,content_version TEXT NOT NULL,operation_lineage TEXT NOT NULL,source_lineage TEXT NOT NULL DEFAULT 'native',correction_of TEXT);
CREATE TABLE IF NOT EXISTS reviewer_findings(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL CHECK(schema_version=1),state TEXT NOT NULL CHECK(state IN ('open','acknowledged','fixed','waived','rejected','stale')),source_lineage TEXT NOT NULL DEFAULT 'native',correction_of TEXT);
CREATE TABLE IF NOT EXISTS outcomes(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),outcome TEXT NOT NULL CHECK(outcome IN ('verified','accepted_with_waivers','accepted_partial','blocked','aborted','failed')),source_lineage TEXT NOT NULL DEFAULT 'native',correction_of TEXT);
CREATE TABLE IF NOT EXISTS retrospectives(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),sequence INTEGER NOT NULL CHECK(sequence >= 0),source_lineage TEXT NOT NULL DEFAULT 'native',correction_of TEXT);
CREATE TABLE IF NOT EXISTS acceptance_requirements(id TEXT PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),sequence INTEGER NOT NULL CHECK(sequence >= 0),source_lineage TEXT NOT NULL DEFAULT 'native',correction_of TEXT);")?;
    let existing_projection_schema = table_exists(db, "projections")?;
    if existing_projection_schema
        && (!trigger_exists(db, "immutable_projections_insert")?
            || !trigger_exists(db, "immutable_projection_rebuild_status_insert")?)
    {
        return Err(Error::AmbiguousSchema);
    }
    db.execute_batch("CREATE TABLE IF NOT EXISTS projections(id TEXT PRIMARY KEY,projection_schema_version INTEGER NOT NULL,event_history_generation INTEGER NOT NULL,source_generation INTEGER NOT NULL,authoritative INTEGER NOT NULL CHECK(authoritative IN (0,1)),status TEXT NOT NULL CHECK(status IN ('missing','stale','drifted','complete','failed')),payload TEXT NOT NULL DEFAULT ''); CREATE TABLE IF NOT EXISTS projection_rebuilds(id INTEGER PRIMARY KEY,projection_id TEXT NOT NULL,event_id TEXT NOT NULL,event_sequence INTEGER NOT NULL,status TEXT NOT NULL CHECK(status IN ('complete','failed')),failure TEXT,drift TEXT); CREATE TABLE IF NOT EXISTS projection_rebuild_status(projection_id TEXT PRIMARY KEY,status TEXT NOT NULL CHECK(status IN ('missing','stale','drifted','complete','failed')),authoritative INTEGER NOT NULL CHECK(authoritative IN (0,1)),failure TEXT,drift TEXT); CREATE TABLE IF NOT EXISTS playback_results(id TEXT PRIMARY KEY,event_history_generation INTEGER NOT NULL,result TEXT NOT NULL CHECK(result IN ('exact_playback_succeeded','diverged')),divergence TEXT,exact INTEGER NOT NULL CHECK(exact IN (0,1))); CREATE TABLE IF NOT EXISTS captured_outcome_simulations(id TEXT PRIMARY KEY,label TEXT NOT NULL CHECK(label='captured_outcome_simulation'),outcome TEXT NOT NULL,exact_playback_evidence INTEGER NOT NULL DEFAULT 0 CHECK(exact_playback_evidence=0));")?;
    db.execute_batch("CREATE TRIGGER IF NOT EXISTS immutable_playback_results_coherence_insert BEFORE INSERT ON playback_results WHEN NOT ((NEW.result='exact_playback_succeeded' AND NEW.exact=1 AND NEW.divergence IS NULL) OR (NEW.result='diverged' AND NEW.exact=0 AND NEW.divergence IS NOT NULL)) BEGIN SELECT RAISE(ABORT,'incoherent playback result'); END; CREATE TRIGGER IF NOT EXISTS immutable_playback_results_coherence_update BEFORE UPDATE ON playback_results WHEN NOT ((NEW.result='exact_playback_succeeded' AND NEW.exact=1 AND NEW.divergence IS NULL) OR (NEW.result='diverged' AND NEW.exact=0 AND NEW.divergence IS NOT NULL)) BEGIN SELECT RAISE(ABORT,'incoherent playback result'); END; CREATE TRIGGER IF NOT EXISTS immutable_projections_insert BEFORE INSERT ON projections WHEN NEW.status != 'complete' AND NEW.authoritative != 0 BEGIN SELECT RAISE(ABORT,'incoherent projection authority'); END; CREATE TRIGGER IF NOT EXISTS immutable_projections_update BEFORE UPDATE ON projections WHEN NEW.status != 'complete' AND NEW.authoritative != 0 BEGIN SELECT RAISE(ABORT,'incoherent projection authority'); END; CREATE TRIGGER IF NOT EXISTS immutable_projection_rebuild_status_insert BEFORE INSERT ON projection_rebuild_status WHEN NEW.status != 'complete' AND NEW.authoritative != 0 BEGIN SELECT RAISE(ABORT,'incoherent projection authority'); END; CREATE TRIGGER IF NOT EXISTS immutable_projection_rebuild_status_update BEFORE UPDATE ON projection_rebuild_status WHEN NEW.status != 'complete' AND NEW.authoritative != 0 BEGIN SELECT RAISE(ABORT,'incoherent projection authority'); END;")?;
    db.execute_batch("CREATE TRIGGER IF NOT EXISTS immutable_projection_rebuilds_update BEFORE UPDATE ON projection_rebuilds BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER IF NOT EXISTS immutable_projection_rebuilds_delete BEFORE DELETE ON projection_rebuilds BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER IF NOT EXISTS immutable_playback_results_update BEFORE UPDATE ON playback_results BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER IF NOT EXISTS immutable_playback_results_delete BEFORE DELETE ON playback_results BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER IF NOT EXISTS immutable_captured_outcome_simulations_update BEFORE UPDATE ON captured_outcome_simulations BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER IF NOT EXISTS immutable_captured_outcome_simulations_delete BEFORE DELETE ON captured_outcome_simulations BEGIN SELECT RAISE(ABORT,'immutable'); END;")?;
    for table in [
        "projections",
        "projection_rebuilds",
        "projection_rebuild_status",
        "playback_results",
        "captured_outcome_simulations",
    ] {
        for (column, definition) in [
            ("schema_version", "INTEGER NOT NULL DEFAULT 1"),
            ("source_lineage", "TEXT NOT NULL DEFAULT 'native'"),
        ] {
            if db.query_row(
                &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name='{column}'"),
                [],
                |r| r.get::<_, i64>(0),
            )? == 0
            {
                db.execute_batch(&format!(
                    "ALTER TABLE {table} ADD COLUMN {column} {definition}"
                ))?;
            }
        }
    }
    for table in [
        "projections",
        "projection_rebuilds",
        "projection_rebuild_status",
        "playback_results",
        "captured_outcome_simulations",
    ] {
        for (column, default) in [("schema_version", "1"), ("source_lineage", "'native'")] {
            let (not_null, actual): (i64, Option<String>) = db.query_row(&format!("SELECT \"notnull\",dflt_value FROM pragma_table_info('{table}') WHERE name='{column}'"), [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if not_null != 1 || actual.as_deref() != Some(default) {
                return Err(Error::AmbiguousSchema);
            }
        }
    }
    for (name, ty) in [
        ("event_sequence", "INTEGER NOT NULL DEFAULT 0"),
        ("transition", "TEXT NOT NULL DEFAULT ''"),
        ("deterministic_input", "TEXT NOT NULL DEFAULT ''"),
        ("outcome", "TEXT"),
    ] {
        if db.query_row(
            &format!("SELECT count(*) FROM pragma_table_info('events') WHERE name='{name}'"),
            [],
            |r| r.get::<_, i64>(0),
        )? == 0
        {
            db.execute_batch(&format!("ALTER TABLE events ADD COLUMN {name} {ty}"))?;
        }
    }
    for table in [
        "events",
        "findings",
        "evidence",
        "history",
        "replay_metadata",
        "lifecycle_evidence",
        "reviewer_findings",
        "outcomes",
        "retrospectives",
        "acceptance_requirements",
    ] {
        if !matches!(
            table,
            "projections" | "projection_rebuild_status" | "events"
        ) {
            db.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS immutable_{table}_update BEFORE UPDATE ON {table} BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER IF NOT EXISTS immutable_{table}_delete BEFORE DELETE ON {table} BEGIN SELECT RAISE(ABORT,'immutable'); END;"))?;
        }
        if db.query_row(
            &format!("SELECT count(*) FROM {table} WHERE schema_version != 1 OR source_lineage=''"),
            [],
            |r| r.get::<_, i64>(0),
        )? != 0
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    validate_slice3_guards(db)?;
    if !had_events {
        db.execute_batch("CREATE TRIGGER immutable_events_update BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER immutable_events_delete BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT,'immutable'); END;")?;
    }
    for name in ["immutable_events_update", "immutable_events_delete"] {
        let valid: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        let valid = valid.is_some_and(|sql| {
            let sql: String = sql
                .chars()
                .filter(|c| !c.is_whitespace())
                .flat_map(char::to_lowercase)
                .collect();
            sql.contains("before")
                && sql.contains("onevents")
                && sql.contains("raise(abort,'immutable')")
                && !sql.contains("when")
        });
        if !valid {
            return Err(Error::AmbiguousSchema);
        }
    }
    Ok(())
}

fn table_exists(db: &Transaction<'_>, n: &str) -> Result<bool, Error> {
    Ok(db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?",
        [n],
        |r| r.get::<_, i64>(0),
    )? != 0)
}

fn trigger_exists(db: &Transaction<'_>, n: &str) -> Result<bool, Error> {
    Ok(db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name=?",
        [n],
        |r| r.get::<_, i64>(0),
    )? != 0)
}

fn index_exists(db: &Transaction<'_>, n: &str) -> Result<bool, Error> {
    Ok(db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='index' AND name=?",
        [n],
        |r| r.get::<_, i64>(0),
    )? == 1)
}

fn nonempty(db: &Transaction<'_>) -> Result<bool, Error> {
    Ok(db.query_row("SELECT count(*) FROM sqlite_master WHERE type IN ('table','index','trigger','view') AND name NOT LIKE 'sqlite_%'",[],|r|r.get::<_,i64>(0))?>0)
}

fn validate_legacy_core(db: &Transaction<'_>) -> Result<(), Error> {
    for (table, required) in [
        (
            "artifacts",
            &["id", "owner", "source", "schema_version"][..],
        ),
        (
            "versions",
            &[
                "id",
                "artifact_id",
                "hash",
                "canonical",
                "operation",
                "actor",
                "source",
                "schema_version",
            ][..],
        ),
        (
            "operations",
            &["id", "fingerprint", "result", "schema_version"][..],
        ),
        (
            "lineage",
            &["operation_id", "version_id", "schema_version"][..],
        ),
        (
            "discrepancies",
            &[
                "id",
                "operation",
                "actor",
                "artifact_id",
                "source",
                "reason",
                "status",
                "operation_result_id",
                "context",
                "schema_version",
            ][..],
        ),
    ] {
        if !table_exists(db, table)?
            || required.iter().any(|name| {
                db.query_row(
                    &format!(
                        "SELECT count(*) FROM pragma_table_info('{table}') WHERE name='{name}'"
                    ),
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
                    != 1
            })
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    Ok(())
}

fn validate_core(db: &Transaction<'_>, context: ValidationContext) -> Result<(), Error> {
    let v3_compat = columns(db, "compatibility_decisions")
        .map(|columns| columns.iter().any(|column| column == "legacy_fingerprint"))
        .unwrap_or(false);
    for t in [
        "schema_metadata",
        "artifacts",
        "versions",
        "operations",
        "lineage",
        "discrepancies",
    ] {
        if !table_exists(db, t)? {
            return Err(Error::AmbiguousSchema);
        }
    }
    for (table, columns) in [
        ("schema_metadata", vec!["key", "value"]),
        ("artifacts", vec!["id", "owner", "source", "schema_version"]),
        (
            "versions",
            vec![
                "id",
                "artifact_id",
                "hash",
                "canonical",
                "ancestry",
                "operation",
                "actor",
                "source",
                "observed_identity",
                "expected_hash",
                "schema_version",
                "source_schema_version",
                "source_lineage",
                "logical_path",
                "git_applicability",
                "git_repository_path",
                "git_head_state",
                "git_head_oid",
                "git_index_present",
                "git_index_fingerprint",
            ],
        ),
        (
            "operations",
            vec![
                "id",
                "fingerprint",
                "result",
                "version_id",
                "lineage",
                "rejection_code",
                "schema_version",
            ],
        ),
        (
            "lineage",
            vec![
                "operation_id",
                "version_id",
                "parent_version_id",
                "schema_version",
            ],
        ),
        (
            "discrepancies",
            vec![
                "id",
                "operation",
                "actor",
                "artifact_id",
                "source",
                "ancestry",
                "reason",
                "status",
                "operation_result_id",
                "expected_hash",
                "observed_hash",
                "context",
                "expected_identity",
                "observed_identity",
                "schema_version",
                "proposed_hash",
                "observed_owner",
                "observed_source",
                "observed_ancestry",
            ],
        ),
        (
            "projections",
            vec![
                "id",
                "projection_schema_version",
                "event_history_generation",
                "source_generation",
                "authoritative",
                "status",
                "payload",
                "schema_version",
                "source_lineage",
            ],
        ),
        (
            "projection_rebuilds",
            vec![
                "id",
                "projection_id",
                "event_id",
                "event_sequence",
                "status",
                "failure",
                "drift",
                "schema_version",
                "source_lineage",
            ],
        ),
        (
            "projection_rebuild_status",
            vec![
                "projection_id",
                "status",
                "authoritative",
                "failure",
                "drift",
                "schema_version",
                "source_lineage",
            ],
        ),
        (
            "playback_results",
            vec![
                "id",
                "event_history_generation",
                "result",
                "divergence",
                "exact",
                "schema_version",
                "source_lineage",
            ],
        ),
        (
            "captured_outcome_simulations",
            vec![
                "id",
                "label",
                "outcome",
                "exact_playback_evidence",
                "schema_version",
                "source_lineage",
            ],
        ),
    ] {
        let actual: Vec<String> = db
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if actual != columns {
            return Err(Error::AmbiguousSchema);
        }
        let expected_types: &[(&str, &str, i64, i64)] = match table {
            "schema_metadata" => &[("key", "TEXT", 0, 1), ("value", "INTEGER", 1, 0)],
            "artifacts" => &[
                ("id", "TEXT", 0, 1),
                ("owner", "TEXT", 1, 0),
                ("source", "TEXT", 1, 0),
                ("schema_version", "INTEGER", 1, 0),
            ],
            "versions" => &[
                ("id", "TEXT", 0, 1),
                ("artifact_id", "TEXT", 1, 0),
                ("hash", "TEXT", 1, 0),
                ("canonical", "TEXT", 1, 0),
                ("ancestry", "TEXT", 0, 0),
                ("operation", "TEXT", 1, 0),
                ("actor", "TEXT", 1, 0),
                ("source", "TEXT", 1, 0),
                ("observed_identity", "TEXT", 0, 0),
                ("expected_hash", "TEXT", 0, 0),
                ("schema_version", "INTEGER", 1, 0),
                ("source_schema_version", "INTEGER", 1, 0),
                ("source_lineage", "TEXT", 1, 0),
                ("logical_path", "BLOB", 0, 0),
                ("git_applicability", "TEXT", 0, 0),
                ("git_repository_path", "BLOB", 0, 0),
                ("git_head_state", "TEXT", 0, 0),
                ("git_head_oid", "TEXT", 0, 0),
                ("git_index_present", "INTEGER", 0, 0),
                ("git_index_fingerprint", "TEXT", 0, 0),
            ],
            "operations" => &[
                ("id", "TEXT", 0, 1),
                ("fingerprint", "TEXT", 1, 0),
                ("result", "TEXT", 1, 0),
                ("version_id", "TEXT", 0, 0),
                ("lineage", "TEXT", 0, 0),
                ("rejection_code", "TEXT", 0, 0),
                ("schema_version", "INTEGER", 1, 0),
            ],
            "lineage" => &[
                ("operation_id", "TEXT", 0, 1),
                ("version_id", "TEXT", 1, 0),
                ("parent_version_id", "TEXT", 0, 0),
                ("schema_version", "INTEGER", 1, 0),
            ],
            "discrepancies" => &[
                ("id", "INTEGER", 0, 1),
                ("operation", "TEXT", 1, 0),
                ("actor", "TEXT", 1, 0),
                ("artifact_id", "TEXT", 1, 0),
                ("source", "TEXT", 1, 0),
                ("ancestry", "TEXT", 0, 0),
                ("reason", "TEXT", 1, 0),
                ("status", "TEXT", 1, 0),
                ("operation_result_id", "TEXT", 1, 0),
                ("expected_hash", "TEXT", 0, 0),
                ("observed_hash", "TEXT", 0, 0),
                ("context", "TEXT", 1, 0),
                ("expected_identity", "TEXT", 0, 0),
                ("observed_identity", "TEXT", 0, 0),
                ("schema_version", "INTEGER", 1, 0),
                ("proposed_hash", "TEXT", 0, 0),
                ("observed_owner", "TEXT", 0, 0),
                ("observed_source", "TEXT", 0, 0),
                ("observed_ancestry", "TEXT", 0, 0),
            ],
            "compatibility_decisions" if v3_compat => &[
                ("id", "INTEGER", 0, 1),
                ("source_schema_version", "INTEGER", 1, 0),
                ("target_schema_version", "INTEGER", 1, 0),
                ("operation", "TEXT", 1, 0),
                ("actor", "TEXT", 1, 0),
                ("source", "TEXT", 1, 0),
                ("outcome", "TEXT", 1, 0),
                ("result", "TEXT", 1, 0),
                ("legacy_fingerprint", "TEXT", 0, 0),
                ("scope_id", "TEXT", 1, 0),
                ("fingerprint_version", "INTEGER", 1, 0),
                ("fingerprint", "TEXT", 1, 0),
                ("created_at", "TEXT", 1, 0),
            ],
            "compatibility_decisions" => &[
                ("id", "INTEGER", 0, 1),
                ("source_schema_version", "INTEGER", 1, 0),
                ("target_schema_version", "INTEGER", 1, 0),
                ("operation", "TEXT", 1, 0),
                ("actor", "TEXT", 1, 0),
                ("source", "TEXT", 1, 0),
                ("outcome", "TEXT", 1, 0),
                ("fingerprint", "TEXT", 1, 0),
                ("result", "TEXT", 1, 0),
            ],
            "projections" => &[
                ("id", "TEXT", 0, 1),
                ("projection_schema_version", "INTEGER", 1, 0),
                ("event_history_generation", "INTEGER", 1, 0),
                ("source_generation", "INTEGER", 1, 0),
                ("authoritative", "INTEGER", 1, 0),
                ("status", "TEXT", 1, 0),
                ("payload", "TEXT", 1, 0),
                ("schema_version", "INTEGER", 1, 0),
                ("source_lineage", "TEXT", 1, 0),
            ],
            "projection_rebuilds" => &[
                ("id", "INTEGER", 0, 1),
                ("projection_id", "TEXT", 1, 0),
                ("event_id", "TEXT", 1, 0),
                ("event_sequence", "INTEGER", 1, 0),
                ("status", "TEXT", 1, 0),
                ("failure", "TEXT", 0, 0),
                ("drift", "TEXT", 0, 0),
                ("schema_version", "INTEGER", 1, 0),
                ("source_lineage", "TEXT", 1, 0),
            ],
            "projection_rebuild_status" => &[
                ("projection_id", "TEXT", 0, 1),
                ("status", "TEXT", 1, 0),
                ("authoritative", "INTEGER", 1, 0),
                ("failure", "TEXT", 0, 0),
                ("drift", "TEXT", 0, 0),
                ("schema_version", "INTEGER", 1, 0),
                ("source_lineage", "TEXT", 1, 0),
            ],
            "playback_results" => &[
                ("id", "TEXT", 0, 1),
                ("event_history_generation", "INTEGER", 1, 0),
                ("result", "TEXT", 1, 0),
                ("divergence", "TEXT", 0, 0),
                ("exact", "INTEGER", 1, 0),
                ("schema_version", "INTEGER", 1, 0),
                ("source_lineage", "TEXT", 1, 0),
            ],
            "captured_outcome_simulations" => &[
                ("id", "TEXT", 0, 1),
                ("label", "TEXT", 1, 0),
                ("outcome", "TEXT", 1, 0),
                ("exact_playback_evidence", "INTEGER", 1, 0),
                ("schema_version", "INTEGER", 1, 0),
                ("source_lineage", "TEXT", 1, 0),
            ],
            _ => return Err(Error::AmbiguousSchema),
        };
        let mut q = db.prepare(&format!(
            "SELECT name,type,\"notnull\",pk FROM pragma_table_info('{table}') ORDER BY cid"
        ))?;
        let actual_types: Vec<(String, String, i64, i64)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        if actual_types
            .iter()
            .map(|(n, t, nn, pk)| (n.as_str(), t.as_str(), *nn, *pk))
            .collect::<Vec<_>>()
            != expected_types
                .iter()
                .map(|(n, t, nn, pk)| (*n, *t, *nn, *pk))
                .collect::<Vec<_>>()
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    let event_columns: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('events') ORDER BY cid")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if event_columns
        != vec![
            "id",
            "schema_version",
            "source_lineage",
            "correction_of",
            "event_sequence",
            "transition",
            "deterministic_input",
            "outcome",
        ]
    {
        return Err(Error::AmbiguousSchema);
    }
    let event_types: Vec<(String, String, i64, i64)> = db
        .prepare("SELECT name,type,\"notnull\",pk FROM pragma_table_info('events') ORDER BY cid")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    if event_types
        != vec![
            ("id".into(), "TEXT".into(), 0, 1),
            ("schema_version".into(), "INTEGER".into(), 1, 0),
            ("source_lineage".into(), "TEXT".into(), 1, 0),
            ("correction_of".into(), "TEXT".into(), 0, 0),
            ("event_sequence".into(), "INTEGER".into(), 1, 0),
            ("transition".into(), "TEXT".into(), 1, 0),
            ("deterministic_input".into(), "TEXT".into(), 1, 0),
            ("outcome".into(), "TEXT".into(), 0, 0),
        ]
    {
        return Err(Error::AmbiguousSchema);
    }
    let event_sql: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='events'",
        [],
        |r| r.get(0),
    )?;
    let event_sql: String = event_sql
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if !event_sql.contains("check(schema_version=1)") {
        return Err(Error::AmbiguousSchema);
    }
    for name in ["immutable_events_update", "immutable_events_delete"] {
        let sql: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        if !sql.is_some_and(|sql| {
            let sql: String = sql
                .chars()
                .filter(|c| !c.is_whitespace())
                .flat_map(char::to_lowercase)
                .collect();
            sql.contains("before")
                && sql.contains("onevents")
                && sql.contains("raise(abort,'immutable')")
                && !sql.contains("when")
        }) {
            return Err(Error::AmbiguousSchema);
        }
    }
    for (table, required) in [
        (
            "projections",
            [
                "check(authoritativein(0,1))",
                "check(statusin('missing','stale','drifted','complete','failed'))",
            ]
            .as_slice(),
        ),
        (
            "projection_rebuilds",
            ["check(statusin('complete','failed'))"].as_slice(),
        ),
        (
            "projection_rebuild_status",
            [
                "check(authoritativein(0,1))",
                "check(statusin('missing','stale','drifted','complete','failed'))",
            ]
            .as_slice(),
        ),
        (
            "playback_results",
            [
                "check(resultin('exact_playback_succeeded','diverged'))",
                "check(exactin(0,1))",
            ]
            .as_slice(),
        ),
        (
            "captured_outcome_simulations",
            [
                "check(label='captured_outcome_simulation')",
                "check(exact_playback_evidence=0)",
            ]
            .as_slice(),
        ),
    ] {
        let sql: String = db.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name=?",
            [table],
            |r| r.get(0),
        )?;
        let normalized: String = sql
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect();
        if required
            .iter()
            .any(|fragment| !normalized.contains(fragment))
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    for (name, table, event) in [
        (
            "immutable_projection_rebuilds_update",
            "projection_rebuilds",
            "UPDATE",
        ),
        (
            "immutable_projection_rebuilds_delete",
            "projection_rebuilds",
            "DELETE",
        ),
        (
            "immutable_playback_results_update",
            "playback_results",
            "UPDATE",
        ),
        (
            "immutable_playback_results_delete",
            "playback_results",
            "DELETE",
        ),
        (
            "immutable_captured_outcome_simulations_update",
            "captured_outcome_simulations",
            "UPDATE",
        ),
        (
            "immutable_captured_outcome_simulations_delete",
            "captured_outcome_simulations",
            "DELETE",
        ),
    ] {
        let sql: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        let valid = sql.as_ref().is_some_and(|sql| {
            let normalized: String = sql
                .chars()
                .filter(|c| !c.is_whitespace())
                .flat_map(char::to_lowercase)
                .collect();
            normalized.contains(&format!("before{}on{}", event.to_lowercase(), table))
                && normalized.contains("raise(abort,'immutable')")
                && !normalized.contains("when")
        });
        if !valid {
            return Err(Error::AmbiguousSchema);
        }
    }
    let compatibility_indexes = if v3_compat {
        [
            ("schema_metadata", vec![vec!["key"]]),
            ("artifacts", vec![vec!["id"]]),
            ("versions", vec![vec!["id"], vec!["operation"]]),
            ("operations", vec![vec!["id"]]),
            ("lineage", vec![vec!["operation_id"]]),
            ("discrepancies", vec![vec!["operation"]]),
            (
                "compatibility_decisions",
                vec![
                    vec![
                        "source_schema_version",
                        "target_schema_version",
                        "operation",
                        "scope_id",
                    ],
                    vec!["fingerprint_version", "fingerprint"],
                ],
            ),
        ]
    } else {
        [
            ("schema_metadata", vec![vec!["key"]]),
            ("artifacts", vec![vec!["id"]]),
            ("versions", vec![vec!["id"], vec!["operation"]]),
            ("operations", vec![vec!["id"]]),
            ("lineage", vec![vec!["operation_id"]]),
            ("discrepancies", vec![vec!["operation"]]),
            ("compatibility_decisions", vec![vec!["fingerprint"]]),
        ]
    };
    for (table, expected) in [
        (
            "versions",
            vec![("versions", "ancestry"), ("artifacts", "artifact_id")],
        ),
        ("operations", vec![("versions", "version_id")]),
        (
            "lineage",
            vec![
                ("versions", "parent_version_id"),
                ("versions", "version_id"),
                ("operations", "operation_id"),
            ],
        ),
    ] {
        if table == "compatibility_decisions"
            && columns(db, table)?
                .iter()
                .any(|column| column == "legacy_fingerprint")
        {
            continue;
        }
        let mut q = db.prepare(&format!(
            "SELECT \"table\",\"from\" FROM pragma_foreign_key_list('{table}') ORDER BY \"from\""
        ))?;
        let actual: Vec<(String, String)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut expected = expected
            .into_iter()
            .map(|(t, c)| (t.to_owned(), c.to_owned()))
            .collect::<Vec<_>>();
        expected.sort();
        let mut actual = actual;
        actual.sort();
        if actual != expected {
            return Err(Error::AmbiguousSchema);
        }
    }
    let indexes: Vec<(String, i64, Vec<String>)> = {
        let mut q = db.prepare("SELECT name,\"unique\" FROM pragma_index_list('versions') WHERE origin='c' ORDER BY name")?;
        let indexes = q
            .query_map([], |r| {
                let name: String = r.get(0)?;
                let unique: i64 = r.get(1)?;
                let columns = db
                    .prepare(&format!(
                        "SELECT name FROM pragma_index_info('{}') ORDER BY seqno",
                        name.replace('\'', "''")
                    ))?
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                Ok((name, unique, columns))
            })?
            .collect::<Result<_, _>>()?;
        indexes
    };
    if indexes
        != vec![(
            "versions_artifact".to_owned(),
            0,
            vec!["artifact_id".to_owned()],
        )]
    {
        return Err(Error::AmbiguousSchema);
    }
    for (table, expected) in compatibility_indexes {
        let mut q = db.prepare(&format!(
            "SELECT seq,name FROM pragma_index_list('{table}') WHERE \"unique\"=1 ORDER BY seq"
        ))?;
        let mut actual = q
            .query_map([], |r| {
                let name: String = r.get(1)?;
                db.prepare(&format!(
                    "SELECT name FROM pragma_index_info('{}') ORDER BY seqno",
                    name.replace('\'', "''")
                ))?
                .query_map([], |x| x.get(0))?
                .collect::<Result<Vec<String>, _>>()
            })?
            .collect::<Result<Vec<_>, _>>()?;
        actual.sort();
        let mut expected: Vec<Vec<String>> = expected
            .into_iter()
            .map(|x| x.into_iter().map(str::to_owned).collect())
            .collect();
        expected.sort();
        if actual != expected {
            return Err(Error::AmbiguousSchema);
        }
    }
    for (name, table, event) in [
        ("immutable_versions_update", "versions", "UPDATE"),
        ("immutable_versions_delete", "versions", "DELETE"),
        ("immutable_operations_update", "operations", "UPDATE"),
        ("immutable_operations_delete", "operations", "DELETE"),
        ("immutable_lineage_update", "lineage", "UPDATE"),
        ("immutable_lineage_delete", "lineage", "DELETE"),
        ("immutable_discrepancies_update", "discrepancies", "UPDATE"),
        ("immutable_discrepancies_delete", "discrepancies", "DELETE"),
    ] {
        let sql: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        if !sql.as_ref().is_some_and(|sql| {
            let normalized: String = sql
                .chars()
                .filter(|c| !c.is_whitespace())
                .flat_map(char::to_lowercase)
                .collect();
            normalized.contains(&format!("before{}on{}", event.to_lowercase(), table))
                && normalized.contains("raise(abort,'immutable')")
                && !normalized.contains("when")
        }) {
            return Err(Error::AmbiguousSchema);
        }
    }
    validate_runtime_record_schema(db)?;
    validate_slice3_guards(db)?;
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok"
        || db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get::<_, i64>(0)
        })? != 0
    {
        return Err(Error::AmbiguousSchema);
    }
    let rows = db.prepare("SELECT v.hash,v.canonical,a.id,v.actor,a.owner,v.source,a.source,v.ancestry,p.id,p.artifact_id,p.actor,p.source,v.schema_version,a.schema_version FROM versions v LEFT JOIN artifacts a ON a.id=v.artifact_id LEFT JOIN versions p ON p.id=v.ancestry")?.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,String>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,Option<String>>(8)?,r.get::<_,Option<String>>(9)?,r.get::<_,Option<String>>(10)?,r.get::<_,Option<String>>(11)?,r.get::<_,i64>(12)?,r.get::<_,Option<i64>>(13)?)))?.collect::<Result<Vec<_>,_>>()?;
    if rows.iter().any(
        |(
            stored,
            canonical,
            artifact,
            actor,
            owner,
            source,
            artifact_source,
            ancestry,
            parent,
            parent_artifact,
            parent_actor,
            parent_source,
            version_schema,
            artifact_schema,
        )| {
            stored != &hash(canonical)
                || artifact.is_none()
                || owner.as_deref() != Some(actor)
                || artifact_source.as_deref() != Some(source)
                || ancestry.is_some()
                    && (parent.is_none()
                        || parent_artifact != artifact
                        || parent_actor.as_deref() != Some(actor)
                        || parent_source.as_deref() != Some(source))
                || *version_schema != 1
                || artifact_schema != &Some(1)
        },
    ) {
        return Err(Error::AmbiguousSchema);
    }
    let malformed_operations = db.query_row(
        "SELECT count(*) FROM operations o WHERE o.result NOT IN ('accepted','rejected')
         OR (o.result='accepted' AND (o.version_id IS NULL OR o.rejection_code IS NOT NULL
             OR (SELECT count(*) FROM versions v WHERE v.id=o.version_id AND v.operation=o.id)=0
             OR (SELECT count(*) FROM lineage l WHERE l.operation_id=o.id AND l.version_id=o.version_id AND l.parent_version_id IS o.lineage)=0))
         OR (o.result='rejected' AND (o.version_id IS NOT NULL OR o.lineage IS NOT NULL
             OR o.rejection_code IS NULL
             OR (SELECT count(*) FROM discrepancies d WHERE d.operation=o.id AND d.status='rejected'
                 AND d.operation_result_id=o.id) != 1))
         OR (o.result='accepted' AND EXISTS (SELECT 1 FROM discrepancies d WHERE d.operation=o.id
             AND (d.status!='rejected' OR d.operation_result_id != o.version_id)))",
        [],
        |r| r.get::<_, i64>(0),
    )?;
    if malformed_operations != 0 {
        return Err(Error::AmbiguousSchema);
    }
    let broken_relations = db.query_row(
        "SELECT
           (SELECT count(*) FROM lineage l
            WHERE l.schema_version != 1
               OR (SELECT count(*) FROM operations o WHERE o.id=l.operation_id) != 1
               OR (SELECT count(*) FROM versions v WHERE v.id=l.version_id) != 1
               OR (SELECT count(*) FROM operations o JOIN versions v ON v.id=o.version_id
                   WHERE o.id=l.operation_id AND o.result='accepted' AND v.id=l.version_id
                     AND o.lineage IS l.parent_version_id) != 1)
         + (SELECT count(*) FROM operations o WHERE o.result='accepted'
            AND ((SELECT count(*) FROM versions v WHERE v.id=o.version_id AND v.operation=o.id) != 1
              OR (SELECT count(*) FROM lineage l WHERE l.operation_id=o.id
                    AND l.version_id=o.version_id AND l.parent_version_id IS o.lineage) != 1))
         + (SELECT count(*) FROM versions v
            WHERE (SELECT count(*) FROM operations o WHERE o.result='accepted'
                     AND o.id=v.operation AND o.version_id=v.id AND o.lineage IS v.ancestry) != 1
               OR (SELECT count(*) FROM lineage l WHERE l.operation_id=v.operation
                     AND l.version_id=v.id AND l.parent_version_id IS v.ancestry) != 1)",
        [],
        |r| r.get::<_, i64>(0),
    )?;
    if broken_relations != 0 {
        return Err(Error::AmbiguousSchema);
    }
    if db.query_row("SELECT count(*) FROM discrepancies d LEFT JOIN operations o ON o.id=d.operation WHERE d.schema_version != 1 OR d.status != 'rejected' OR o.id IS NULL OR (o.result='rejected' AND d.operation_result_id != d.operation) OR (o.result='accepted' AND d.operation_result_id != o.version_id)", [], |r| r.get::<_, i64>(0))? != 0 {
        return Err(Error::AmbiguousSchema);
    }
    let metadata: Vec<(String, i64)> = {
        let mut q = db.prepare("SELECT key,value FROM schema_metadata ORDER BY key")?;
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        rows
    };
    let (expected_schema_version, expected_lineage) = context.expected_metadata();
    if metadata
        != vec![
            ("migration_lineage".to_owned(), expected_lineage),
            ("schema_version".to_owned(), expected_schema_version),
        ]
    {
        return Err(Error::AmbiguousSchema);
    }
    if !table_exists(db, "compatibility_decisions")? || db.query_row("SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND name NOT IN ('schema_metadata','artifacts','versions','operations','lineage','discrepancies','compatibility_decisions','versions_artifact','versions_observation_guard','immutable_versions_update','immutable_versions_delete','immutable_operations_update','immutable_operations_delete','immutable_lineage_update','immutable_lineage_delete','immutable_discrepancies_update','immutable_discrepancies_delete','events','findings','evidence','history','replay_metadata','lifecycle_evidence','reviewer_findings','outcomes','retrospectives','acceptance_requirements','projections','projection_rebuilds','projection_rebuild_status','playback_results','captured_outcome_simulations','immutable_projections_insert','immutable_projections_update','immutable_projection_rebuild_status_insert','immutable_projection_rebuild_status_update','immutable_projection_rebuilds_update','immutable_projection_rebuilds_delete','immutable_playback_results_update','immutable_playback_results_delete','immutable_playback_results_coherence_insert','immutable_playback_results_coherence_update','immutable_captured_outcome_simulations_update','immutable_captured_outcome_simulations_delete','immutable_events_update','immutable_events_delete','immutable_findings_update','immutable_findings_delete','immutable_evidence_update','immutable_evidence_delete','immutable_history_update','immutable_history_delete','immutable_replay_metadata_update','immutable_replay_metadata_delete','immutable_lifecycle_evidence_update','immutable_lifecycle_evidence_delete','immutable_reviewer_findings_update','immutable_reviewer_findings_delete','immutable_outcomes_update','immutable_outcomes_delete','immutable_retrospectives_update','immutable_retrospectives_delete','immutable_acceptance_requirements_update','immutable_acceptance_requirements_delete','reconciliation_runs','reconciliation_discrepancies','reconciliation_projection_evidence','recovery_actions','recovery_outcomes','reconciliation_discrepancies_guards','recovery_actions_guards','recovery_outcomes_guards','reconciliation_runs_scope_sequence','reconciliation_discrepancies_run_correction','reconciliation_projection_evidence_run','recovery_actions_fingerprint','recovery_outcomes_action_sequence','reconciliation_runs_immutable_update','reconciliation_runs_immutable_delete','reconciliation_discrepancies_immutable_update','reconciliation_discrepancies_immutable_delete','recovery_actions_immutable_update','recovery_actions_immutable_delete','recovery_outcomes_immutable_update','recovery_outcomes_immutable_delete')", [], |r| r.get::<_, i64>(0))? != 0 { return Err(Error::AmbiguousSchema); }
    if context.target_version < 3 && db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('reconciliation_projection_evidence','reconciliation_projection_evidence_run')", [], |r| r.get::<_, i64>(0))? != 0 { return Err(Error::AmbiguousSchema); }
    for table in [
        "artifacts",
        "versions",
        "operations",
        "lineage",
        "discrepancies",
    ] {
        if db.query_row(
            &format!("SELECT count(*) FROM {table} WHERE schema_version != 1"),
            [],
            |r| r.get::<_, i64>(0),
        )? != 0
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    for table in [
        "projections",
        "projection_rebuilds",
        "projection_rebuild_status",
        "playback_results",
        "captured_outcome_simulations",
    ] {
        if db.query_row(
            &format!("SELECT count(*) FROM {table} WHERE schema_version != 1 OR source_lineage=''"),
            [],
            |r| r.get::<_, i64>(0),
        )? != 0
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    if db.query_row("SELECT count(*) FROM versions WHERE source_schema_version NOT IN (0,1) OR source_lineage NOT IN ('native','legacy') OR (source_schema_version=1 AND source_lineage != 'native') OR (source_schema_version=0 AND source_lineage != 'legacy')", [], |r| r.get::<_, i64>(0))? != 0
        || (!v3_compat && db.query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version NOT IN (0,1) OR target_schema_version NOT IN (1,2) OR outcome NOT IN ('accepted','rejected') OR result NOT IN ('migrated','rejected') OR (outcome='accepted' AND result != 'migrated')", [], |r| r.get::<_, i64>(0))? != 0)
        || db.query_row("SELECT count(*) FROM playback_results WHERE (result='exact_playback_succeeded' AND (exact != 1 OR divergence IS NOT NULL)) OR (result='diverged' AND (exact != 0 OR divergence IS NULL))", [], |r| r.get::<_, i64>(0))? != 0
    {
        return Err(Error::AmbiguousSchema);
    }
    Ok(())
}

fn validate_runtime_record_schema(db: &Transaction<'_>) -> Result<(), Error> {
    let expected = [
        ("events", "id:TEXT:0:1,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0,event_sequence:INTEGER:1:0,transition:TEXT:1:0,deterministic_input:TEXT:1:0,outcome:TEXT:0:0"),
        ("findings", "id:TEXT:0:1,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("evidence", "id:TEXT:0:1,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("history", "id:TEXT:0:1,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("replay_metadata", "id:TEXT:0:1,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("lifecycle_evidence", "id:TEXT:0:1,schema_version:INTEGER:1:0,status:TEXT:1:0,actor:TEXT:1:0,reason:TEXT:1:0,content_version:TEXT:1:0,operation_lineage:TEXT:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("reviewer_findings", "id:TEXT:0:1,schema_version:INTEGER:1:0,state:TEXT:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("outcomes", "id:TEXT:0:1,schema_version:INTEGER:1:0,outcome:TEXT:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("retrospectives", "id:TEXT:0:1,schema_version:INTEGER:1:0,sequence:INTEGER:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("acceptance_requirements", "id:TEXT:0:1,schema_version:INTEGER:1:0,sequence:INTEGER:1:0,source_lineage:TEXT:1:0,correction_of:TEXT:0:0"),
        ("projections", "id:TEXT:0:1,projection_schema_version:INTEGER:1:0,event_history_generation:INTEGER:1:0,source_generation:INTEGER:1:0,authoritative:INTEGER:1:0,status:TEXT:1:0,payload:TEXT:1:0,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0"),
        ("projection_rebuilds", "id:INTEGER:0:1,projection_id:TEXT:1:0,event_id:TEXT:1:0,event_sequence:INTEGER:1:0,status:TEXT:1:0,failure:TEXT:0:0,drift:TEXT:0:0,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0"),
        ("projection_rebuild_status", "projection_id:TEXT:0:1,status:TEXT:1:0,authoritative:INTEGER:1:0,failure:TEXT:0:0,drift:TEXT:0:0,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0"),
        ("playback_results", "id:TEXT:0:1,event_history_generation:INTEGER:1:0,result:TEXT:1:0,divergence:TEXT:0:0,exact:INTEGER:1:0,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0"),
        ("captured_outcome_simulations", "id:TEXT:0:1,label:TEXT:1:0,outcome:TEXT:1:0,exact_playback_evidence:INTEGER:1:0,schema_version:INTEGER:1:0,source_lineage:TEXT:1:0"),
    ];
    for (table, columns) in expected {
        if !table_exists(db, table)? {
            return Err(Error::AmbiguousSchema);
        }
        let actual: Vec<String> = db
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let expected = columns
            .split(',')
            .map(|column| column.split(':').next().unwrap().to_owned())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(Error::AmbiguousSchema);
        }
        let actual_types: Vec<(String, String, i64, i64)> = db
            .prepare(&format!(
                "SELECT name,type,\"notnull\",pk FROM pragma_table_info('{table}') ORDER BY cid"
            ))?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        let expected_types = columns
            .split(',')
            .map(|column| {
                let mut parts = column.split(':');
                (
                    parts.next().unwrap().to_owned(),
                    parts.next().unwrap().to_owned(),
                    parts.next().unwrap().parse::<i64>().unwrap(),
                    parts.next().unwrap().parse::<i64>().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        if actual_types != expected_types {
            return Err(Error::AmbiguousSchema);
        }
        let expected_defaults: &[(&str, Option<&str>)] = match table {
            "events" => &[
                ("id", None),
                ("schema_version", None),
                ("source_lineage", None),
                ("correction_of", None),
                ("event_sequence", Some("0")),
                ("transition", Some("''")),
                ("deterministic_input", Some("''")),
                ("outcome", None),
            ],
            "lifecycle_evidence" => &[
                ("id", None),
                ("schema_version", Some("1")),
                ("status", None),
                ("actor", None),
                ("reason", None),
                ("content_version", None),
                ("operation_lineage", None),
                ("source_lineage", Some("'native'")),
                ("correction_of", None),
            ],
            "reviewer_findings" => &[
                ("id", None),
                ("schema_version", None),
                ("state", None),
                ("source_lineage", Some("'native'")),
                ("correction_of", None),
            ],
            "outcomes" => &[
                ("id", None),
                ("schema_version", None),
                ("outcome", None),
                ("source_lineage", Some("'native'")),
                ("correction_of", None),
            ],
            "projections" => &[
                ("id", None),
                ("projection_schema_version", None),
                ("event_history_generation", None),
                ("source_generation", None),
                ("authoritative", None),
                ("status", None),
                ("payload", Some("''")),
                ("schema_version", Some("1")),
                ("source_lineage", Some("'native'")),
            ],
            "projection_rebuilds" => &[
                ("id", None),
                ("projection_id", None),
                ("event_id", None),
                ("event_sequence", None),
                ("status", None),
                ("failure", None),
                ("drift", None),
                ("schema_version", Some("1")),
                ("source_lineage", Some("'native'")),
            ],
            "projection_rebuild_status" => &[
                ("projection_id", None),
                ("status", None),
                ("authoritative", None),
                ("failure", None),
                ("drift", None),
                ("schema_version", Some("1")),
                ("source_lineage", Some("'native'")),
            ],
            "playback_results" => &[
                ("id", None),
                ("event_history_generation", None),
                ("result", None),
                ("divergence", None),
                ("exact", None),
                ("schema_version", Some("1")),
                ("source_lineage", Some("'native'")),
            ],
            "captured_outcome_simulations" => &[
                ("id", None),
                ("label", None),
                ("outcome", None),
                ("exact_playback_evidence", Some("0")),
                ("schema_version", Some("1")),
                ("source_lineage", Some("'native'")),
            ],
            "retrospectives" | "acceptance_requirements" => &[
                ("id", None),
                ("schema_version", None),
                ("sequence", None),
                ("source_lineage", Some("'native'")),
                ("correction_of", None),
            ],
            _ => &[],
        };
        // Physical defaults belong to the versioned canonical comparator.
        let _ = expected_defaults;
        if table == "events" {
            let sql: String = db.query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?",
                [table],
                |r| r.get(0),
            )?;
            if !sql
                .to_lowercase()
                .replace(char::is_whitespace, "")
                .contains("check(schema_version=1)")
            {
                return Err(Error::AmbiguousSchema);
            }
        }
    }
    for (table, fragment) in [
        ("events", "check(schema_version=1)"),
        ("findings", "check(schema_version=1)"),
        ("evidence", "check(schema_version=1)"),
        ("history", "check(schema_version=1)"),
        ("replay_metadata", "check(schema_version=1)"),
        ("lifecycle_evidence", "check(schema_version=1)"),
        ("reviewer_findings", "check(schema_version=1)"),
        ("outcomes", "check(schema_version=1)"),
        ("lifecycle_evidence", "check(statusin('created','invalidated'))"),
        ("reviewer_findings", "check(statein('open','acknowledged','fixed','waived','rejected','stale'))"),
        ("outcomes", "check(outcomein('verified','accepted_with_waivers','accepted_partial','blocked','aborted','failed'))"),
        ("projections", "check(authoritativein(0,1))"),
        ("projection_rebuilds", "check(statusin('complete','failed'))"),
        ("projection_rebuild_status", "check(authoritativein(0,1))"),
        ("playback_results", "check(resultin('exact_playback_succeeded','diverged'))"),
        ("playback_results", "check(exactin(0,1))"),
        ("captured_outcome_simulations", "check(label='captured_outcome_simulation')"),
        ("captured_outcome_simulations", "check(exact_playback_evidence=0)"),
    ] {
        let sql: String = db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?", [table], |r| r.get(0))?;
        if !sql.to_lowercase().replace(char::is_whitespace, "").contains(fragment) {
            return Err(Error::AmbiguousSchema);
        }
    }
    for table in [
        "events",
        "findings",
        "evidence",
        "history",
        "replay_metadata",
        "lifecycle_evidence",
        "reviewer_findings",
        "outcomes",
        "retrospectives",
        "acceptance_requirements",
        "projection_rebuilds",
        "playback_results",
        "captured_outcome_simulations",
    ] {
        for operation in ["update", "delete"] {
            let name = format!("immutable_{table}_{operation}");
            let sql: Option<String> = db
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?",
                    [&name],
                    |r| r.get(0),
                )
                .optional()?;
            if !sql.is_some_and(|sql| {
                let sql = sql.to_lowercase().replace(char::is_whitespace, "");
                sql.contains(&format!("before{}on{}", operation, table))
                    && sql.contains("raise(abort,'immutable')")
                    && !sql.contains("when")
            }) {
                return Err(Error::AmbiguousSchema);
            }
        }
    }
    for table in ["retrospectives", "acceptance_requirements"] {
        if db.query_row(
            &format!("SELECT count(*) FROM {table} WHERE sequence < 0"),
            [],
            |r| r.get::<_, i64>(0),
        )? != 0
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    for name in [
        "immutable_playback_results_coherence_insert",
        "immutable_playback_results_coherence_update",
    ] {
        let sql: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        if !sql.is_some_and(|sql| {
            let sql = sql.to_lowercase().replace(char::is_whitespace, "");
            sql.contains("before")
                && sql.contains("onplayback_results")
                && sql.contains("new.result='exact_playback_succeeded'")
                && sql.contains("new.result='diverged'")
                && sql.contains("raise(abort")
        }) {
            return Err(Error::AmbiguousSchema);
        }
    }
    Ok(())
}

fn migrate(db: &Transaction<'_>) -> Result<(), Error> {
    if !table_exists(db, "artifacts")? || !table_exists(db, "versions")? {
        return Err(Error::AmbiguousSchema);
    };
    let mut objects: Vec<String> = db
        .prepare("SELECT type||':'||name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    objects.sort();
    if objects != vec!["table:artifacts", "table:schema_metadata", "table:versions"] {
        return Err(Error::AmbiguousSchema);
    }
    let metadata: Vec<(String, i64)> = db
        .prepare("SELECT key,value FROM schema_metadata ORDER BY key")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    if metadata
        != vec![
            ("migration_lineage".to_owned(), 0),
            ("schema_version".to_owned(), 0),
        ]
    {
        return Err(Error::AmbiguousSchema);
    }
    for (table, columns) in [
        ("schema_metadata", vec!["key", "value"]),
        ("artifacts", vec!["id", "owner", "source"]),
        (
            "versions",
            vec![
                "id",
                "artifact_id",
                "hash",
                "canonical",
                "ancestry",
                "operation",
                "actor",
                "source",
            ],
        ),
    ] {
        let actual: Vec<String> = db
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if actual != columns {
            return Err(Error::AmbiguousSchema);
        }
        let expected_types: &[(&str, &str, i64, i64)] = match table {
            "schema_metadata" => &[("key", "TEXT", 0, 1), ("value", "INTEGER", 1, 0)],
            "artifacts" => &[
                ("id", "TEXT", 0, 1),
                ("owner", "TEXT", 1, 0),
                ("source", "TEXT", 1, 0),
            ],
            "versions" => &[
                ("id", "TEXT", 0, 1),
                ("artifact_id", "TEXT", 1, 0),
                ("hash", "TEXT", 1, 0),
                ("canonical", "TEXT", 1, 0),
                ("ancestry", "TEXT", 0, 0),
                ("operation", "TEXT", 1, 0),
                ("actor", "TEXT", 1, 0),
                ("source", "TEXT", 1, 0),
            ],
            _ => return Err(Error::AmbiguousSchema),
        };
        let actual_types: Vec<(String, String, i64, i64)> = db
            .prepare(&format!(
                "SELECT name,type,\"notnull\",pk FROM pragma_table_info('{table}') ORDER BY cid"
            ))?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        if actual_types
            != expected_types
                .iter()
                .map(|(n, t, nn, pk)| (n.to_string(), t.to_string(), *nn, *pk))
                .collect::<Vec<_>>()
        {
            return Err(Error::AmbiguousSchema);
        }
    }
    if db.query_row(
        "SELECT count(*) FROM pragma_foreign_key_list('artifacts')",
        [],
        |r| r.get::<_, i64>(0),
    )? != 0
        || db.query_row(
            "SELECT count(*) FROM pragma_foreign_key_list('versions')",
            [],
            |r| r.get::<_, i64>(0),
        )? != 0
    {
        return Err(Error::AmbiguousSchema);
    }
    let unique_indexes: Vec<Vec<String>> = db
        .prepare(
            "SELECT name FROM pragma_index_list('versions') WHERE \"unique\"=1 AND origin='u'",
        )?
        .query_map([], |r| {
            let name: String = r.get(0)?;
            db.prepare(&format!(
                "SELECT name FROM pragma_index_info('{}') ORDER BY seqno",
                name.replace('\'', "''")
            ))?
            .query_map([], |column| column.get(0))?
            .collect::<Result<Vec<String>, _>>()
        })?
        .collect::<Result<_, _>>()?;
    if unique_indexes != vec![vec!["operation".to_owned()]] {
        return Err(Error::AmbiguousSchema);
    }
    let bad: i64 = db.query_row("SELECT count(*) FROM versions v LEFT JOIN artifacts a ON a.id=v.artifact_id LEFT JOIN versions p ON p.id=v.ancestry WHERE a.id IS NULL OR v.id='' OR (v.ancestry IS NOT NULL AND (p.id IS NULL OR p.artifact_id != v.artifact_id OR p.actor != v.actor OR p.source != v.source)) OR v.actor != a.owner OR v.source != a.source OR v.operation=''", [], |r| r.get(0))?;
    if bad != 0 {
        return Err(Error::AmbiguousSchema);
    }
    let legacy_rows: Vec<LegacyRow> = {
        let mut q = db.prepare(
            "SELECT id,artifact_id,hash,canonical,ancestry,operation,actor,source FROM versions",
        )?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows
    };
    if legacy_rows
        .iter()
        .any(|(_, _, stored, content, _, _, _, _)| stored != &hash(content))
    {
        return Err(Error::AmbiguousSchema);
    }
    db.execute_batch("ALTER TABLE schema_metadata RENAME TO legacy_schema_metadata; ALTER TABLE artifacts RENAME TO legacy_artifacts; ALTER TABLE versions RENAME TO legacy_versions;")?;
    create_schema(db)?;
    db.execute(
        "UPDATE schema_metadata SET value=1 WHERE key='schema_version'",
        [],
    )?;
    db.execute_batch("INSERT INTO artifacts(id,owner,source,schema_version) SELECT id,owner,source,1 FROM legacy_artifacts; INSERT INTO versions(id,artifact_id,hash,canonical,ancestry,operation,actor,source,source_schema_version,source_lineage,schema_version) SELECT id,artifact_id,hash,canonical,ancestry,operation,actor,source,0,'legacy',1 FROM legacy_versions;")?;
    db.execute("INSERT INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,fingerprint,result) VALUES(0,1,'migration','system','artifact-store','accepted','legacy-0-to-1','migrated')", [])?;
    for (id, artifact, _hash, content, ancestry, operation, actor, source) in legacy_rows {
        let fp = fingerprint_parts(
            &actor,
            &source,
            Some(&artifact),
            &hash(&content),
            None,
            ancestry.as_deref(),
        );
        db.execute(
            "INSERT INTO operations VALUES(?,?, 'accepted',?,?,NULL,1)",
            params![operation, fp, id, ancestry],
        )?;
        db.execute(
            "INSERT INTO lineage VALUES(?,?,?,1)",
            params![operation, id, ancestry],
        )?;
    }
    db.execute_batch("DROP TABLE legacy_versions; DROP TABLE legacy_artifacts; DROP TABLE legacy_schema_metadata;").map_err(Error::Sql)
}

pub(super) fn initialize(db: &Transaction<'_>) -> Result<(), Error> {
    let version = if table_exists(db, "schema_metadata")? {
        db.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(|_| Error::AmbiguousSchema)?
    } else {
        None
    };
    match version {
        None if nonempty(db)? => return Err(Error::AmbiguousSchema),
        None => {
            create_schema(db)?;
            prepare_canonical_v2(db)?;
            install_version_observation_columns(db)?;
            db.execute_batch("DROP TABLE compatibility_decisions;")?;
            create_compatibility_v3(db)?;
            ensure_slice4_v3_schema(db)?;
            insert_decision(
                db,
                2,
                3,
                "slice-4-additive-compatibility",
                "system",
                "store",
                "migrated",
                "migrated",
                "store",
                None,
                timestamp(),
            )?;
            db.execute(
                "UPDATE schema_metadata SET value=3 WHERE key='schema_version'",
                [],
            )?;
        }
        Some(0) => {
            migrate(db).map_err(|e| match e {
                Error::Sql(_) => Error::AmbiguousSchema,
                other => other,
            })?;
            db.execute("INSERT OR IGNORE INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,fingerprint,result) VALUES(1,2,'slice-3-additive-compatibility','system','artifact-store','accepted','slice-3-additive-compatibility','migrated')", [])?;
            ensure_runtime_record_schema(db)?;
            db.execute_batch("CREATE TRIGGER IF NOT EXISTS immutable_events_update BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT,'immutable'); END; CREATE TRIGGER IF NOT EXISTS immutable_events_delete BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT,'immutable'); END;")?;
            validate_v2_compatibility(
                db,
                ValidationContext {
                    origin_version: 1,
                    target_version: 3,
                    metadata_finalized: false,
                },
            )?;
            migrate_v2_to_v3(db)?;
            ensure_slice4_v3_schema(db)?;
            validate_canonical_physical_schema(db, 3)?;
            validate_slice4_v3_schema(db)?;
            eprintln!("DIAGNOSTIC validate_slice4_v3_schema passed; next predicate: validate_v3_compatibility");
            validate_v3_compatibility(db)?;
            validate_core(
                db,
                ValidationContext {
                    origin_version: 1,
                    target_version: 3,
                    metadata_finalized: false,
                },
            )?;
            validate_sqlite_integrity(db)?;
            migration_checkpoint()?;
            db.execute(
                "UPDATE schema_metadata SET value=3 WHERE key='schema_version'",
                [],
            )?;
            validate_core(
                db,
                ValidationContext {
                    origin_version: 1,
                    target_version: 3,
                    metadata_finalized: true,
                },
            )?;
        }
        Some(1) => {
            let slice3_present = [
                "projections",
                "projection_rebuilds",
                "projection_rebuild_status",
                "playback_results",
                "captured_outcome_simulations",
            ]
            .iter()
            .try_fold(false, |present, name| {
                Ok::<_, Error>(present || table_exists(db, name)?)
            })?;
            if !slice3_present {
                validate_legacy_core(db)?;
            }
            if table_exists(db, "events")? {
                for name in ["id", "schema_version", "source_lineage", "correction_of"] {
                    if db.query_row(
                        &format!(
                            "SELECT count(*) FROM pragma_table_info('events') WHERE name='{name}'"
                        ),
                        [],
                        |r| r.get::<_, i64>(0),
                    )? != 1
                    {
                        return Err(Error::AmbiguousSchema);
                    }
                }
                let later_event_columns = [
                    ("event_sequence", "INTEGER NOT NULL DEFAULT 0"),
                    ("transition", "TEXT NOT NULL DEFAULT ''"),
                    ("deterministic_input", "TEXT NOT NULL DEFAULT ''"),
                    ("outcome", "TEXT"),
                ];
                let present = later_event_columns
                        .iter()
                        .filter(|(name, _)| {
                            db.query_row(
                                &format!("SELECT count(*) FROM pragma_table_info('events') WHERE name='{name}'"),
                                [],
                                |r| r.get::<_, i64>(0),
                            ).unwrap_or(0) == 1
                        })
                        .count();
                if present != 0 && present != later_event_columns.len() {
                    return Err(Error::AmbiguousSchema);
                }
                if !slice3_present && present == 0 {
                    for (name, definition) in later_event_columns {
                        db.execute(
                            &format!("ALTER TABLE events ADD COLUMN {name} {definition}"),
                            [],
                        )?;
                    }
                }
            }
            if slice3_present {
                validate_core(
                    db,
                    ValidationContext {
                        origin_version: 1,
                        target_version: 3,
                        metadata_finalized: false,
                    },
                )
                .map_err(|e| match e {
                    Error::Sql(_) => Error::AmbiguousSchema,
                    other => other,
                })?;
            }
            if slice3_present || !table_exists(db, "compatibility_decisions")? {
                prepare_canonical_v2(db)?;
            }
            ensure_compatibility_decision_table(db)?;
            db.execute("INSERT INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,fingerprint,result) VALUES(1,2,'slice-3-additive-compatibility','system','artifact-store','accepted','slice-3-additive-compatibility','migrated')", [])?;
            validate_v2_compatibility(
                db,
                ValidationContext {
                    origin_version: 1,
                    target_version: 3,
                    metadata_finalized: false,
                },
            )?;
            migrate_v2_to_v3(db)?;
            ensure_slice4_v3_schema(db)?;
            validate_canonical_physical_schema(db, 3)?;
            validate_slice4_v3_schema(db)?;
            eprintln!("DIAGNOSTIC validate_slice4_v3_schema passed; next predicate: validate_v3_compatibility");
            validate_v3_compatibility(db)?;
            validate_core(
                db,
                ValidationContext {
                    origin_version: 1,
                    target_version: 3,
                    metadata_finalized: false,
                },
            )?;
            validate_sqlite_integrity(db)?;
            migration_checkpoint()?;
            db.execute(
                "UPDATE schema_metadata SET value=3 WHERE key='schema_version'",
                [],
            )?;
            validate_core(
                db,
                ValidationContext {
                    origin_version: 1,
                    target_version: 3,
                    metadata_finalized: true,
                },
            )?;
        }
        Some(2) => {
            validate_canonical_physical_schema(db, 2)?;
            validate_v2_compatibility(
                db,
                ValidationContext {
                    origin_version: 2,
                    target_version: 3,
                    metadata_finalized: false,
                },
            )?;
            migrate_v2_to_v3(db)?;
            ensure_slice4_v3_schema(db)?;
            validate_canonical_physical_schema(db, 3)?;
            validate_slice4_v3_schema(db)?;
            validate_v3_compatibility(db)?;
            validate_core(
                db,
                ValidationContext {
                    origin_version: 2,
                    target_version: 3,
                    metadata_finalized: false,
                },
            )?;
            validate_sqlite_integrity(db)?;
            migration_checkpoint()?;
            db.execute(
                "UPDATE schema_metadata SET value=3 WHERE key='schema_version'",
                [],
            )?;
            validate_core(
                db,
                ValidationContext {
                    origin_version: 2,
                    target_version: 3,
                    metadata_finalized: true,
                },
            )?;
        }
        Some(STORE_SCHEMA) => {
            ensure_slice4_v3_schema(db)?;
            validate_canonical_physical_schema(db, 3)?;
            validate_slice4_v3_schema(db)?;
            validate_v3_compatibility(db)?;
            validate_core(
                db,
                ValidationContext {
                    origin_version: 3,
                    target_version: 3,
                    metadata_finalized: true,
                },
            )?;
            require_slice4_decision(db)?
        }
        Some(v) if v > STORE_SCHEMA => return Err(Error::UnsupportedSchema(v)),
        Some(_) => return Err(Error::AmbiguousSchema),
    }
    if version != Some(STORE_SCHEMA) {
        ensure_record_schema(db)?;
    }
    Ok(())
}

pub(super) fn timestamp() -> String {
    "1970-01-01T00:00:00.000000Z".into()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decision_fingerprint(
    source: i64,
    target: i64,
    operation: &str,
    actor: &str,
    origin: &str,
    outcome: &str,
    result: &str,
    scope: &str,
) -> String {
    let mut bytes = b"akashic/decision/v1\0".to_vec();
    for value in [source.to_be_bytes().to_vec(), target.to_be_bytes().to_vec()].into_iter() {
        bytes.extend(value);
    }
    for value in [operation, actor, origin, outcome, result, scope] {
        bytes.extend((value.len() as u32).to_be_bytes());
        bytes.extend(value.as_bytes());
    }
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
#[allow(dead_code, clippy::too_many_arguments)]
pub(crate) fn test_decision_fingerprint(
    source: i64,
    target: i64,
    operation: &str,
    actor: &str,
    origin: &str,
    outcome: &str,
    result: &str,
    scope: &str,
) -> String {
    decision_fingerprint(
        source, target, operation, actor, origin, outcome, result, scope,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert_decision(
    db: &Transaction<'_>,
    source: i64,
    target: i64,
    operation: &str,
    actor: &str,
    origin: &str,
    outcome: &str,
    result: &str,
    scope: &str,
    legacy: Option<&str>,
    created: String,
) -> Result<(), Error> {
    let fingerprint = decision_fingerprint(
        source, target, operation, actor, origin, outcome, result, scope,
    );
    db.execute("INSERT INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,result,legacy_fingerprint,scope_id,fingerprint_version,fingerprint,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)", params![source,target,operation,actor,origin,outcome,result,legacy,scope,1,fingerprint,created])?;
    Ok(())
}

pub(super) fn migration_checkpoint() -> Result<(), Error> {
    #[cfg(test)]
    if MIGRATION_FAILPOINT.with(|armed| armed.replace(false)) {
        return Err(Error::RebuildFailure);
    }
    Ok(())
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn fail_next_migration_checkpoint() -> MigrationFailpointGuard {
    MIGRATION_FAILPOINT.with(|armed| armed.set(true));
    MigrationFailpointGuard(PhantomData)
}

#[cfg(test)]
pub(crate) struct MigrationFailpointGuard(PhantomData<Rc<()>>);

#[cfg(test)]
impl Drop for MigrationFailpointGuard {
    fn drop(&mut self) {
        MIGRATION_FAILPOINT.with(|armed| armed.set(false));
    }
}

pub(super) fn migrate_v2_to_v3(db: &Transaction<'_>) -> Result<(), Error> {
    if [
        "reconciliation_runs",
        "reconciliation_discrepancies",
        "recovery_actions",
        "recovery_outcomes",
    ]
    .iter()
    .any(|n| table_exists(db, n).unwrap_or(true))
    {
        return Err(Error::AmbiguousSchema);
    }
    for trigger in ["immutable_events_update", "immutable_events_delete"] {
        if !trigger_exists(db, trigger)? {
            return Err(Error::AmbiguousSchema);
        }
    }
    install_version_observation_columns(db)?;
    let event_columns: i64 = db.query_row(
        "SELECT count(*) FROM pragma_table_info('events')",
        [],
        |r| r.get(0),
    )?;
    if event_columns != 8 {
        return Err(Error::AmbiguousSchema);
    }
    // v2 is deliberately narrow: anything beyond the Slice-3 decision shape
    // is transitional/corrupt, not something this migration repairs.
    for column in [
        "id",
        "source_schema_version",
        "target_schema_version",
        "operation",
        "actor",
        "source",
        "outcome",
        "fingerprint",
        "result",
    ] {
        if db.query_row(&format!("SELECT count(*) FROM pragma_table_info('compatibility_decisions') WHERE name='{column}'"), [], |r| r.get::<_, i64>(0))? != 1 {
            return Err(Error::AmbiguousSchema);
        }
    }
    db.execute_batch("ALTER TABLE compatibility_decisions RENAME TO compatibility_decisions_v2;")?;
    create_compatibility_v3(db)?;
    ensure_slice4_v3_schema(db)?;
    let mut rows = db.prepare("SELECT id,source_schema_version,target_schema_version,operation,actor,source,outcome,result,fingerprint FROM compatibility_decisions_v2 ORDER BY id")?;
    let mapped: Vec<MigrationRow> = rows
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })
        .and_then(|it| it.collect())?;
    let mut fingerprints = std::collections::HashSet::new();
    for (id, source, target, operation, actor, origin, outcome, result, legacy) in mapped {
        if !fingerprints.insert(legacy.clone()) {
            return Err(Error::AmbiguousSchema);
        }
        db.execute(
            "INSERT INTO compatibility_decisions(id,source_schema_version,target_schema_version,operation,actor,source,outcome,result,legacy_fingerprint,scope_id,fingerprint_version,fingerprint,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,1,?,?)",
            params![id, source, target, operation, actor, origin, outcome, result, legacy, "store", decision_fingerprint(source, target, &operation, &actor, &origin, &outcome, &result, "store"), timestamp()],
        )?;
    }
    db.execute_batch("DROP TABLE compatibility_decisions_v2;")?;
    insert_decision(
        db,
        2,
        3,
        "slice-4-additive-compatibility",
        "system",
        "store",
        "migrated",
        "migrated",
        "store",
        None,
        timestamp(),
    )?;
    Ok(())
}

pub(super) fn create_compatibility_v3(db: &Transaction<'_>) -> Result<(), Error> {
    db.execute_batch("CREATE TABLE compatibility_decisions(id INTEGER PRIMARY KEY,source_schema_version INTEGER NOT NULL,target_schema_version INTEGER NOT NULL,operation TEXT NOT NULL,actor TEXT NOT NULL,source TEXT NOT NULL,outcome TEXT NOT NULL,result TEXT NOT NULL,legacy_fingerprint TEXT,scope_id TEXT NOT NULL,fingerprint_version INTEGER NOT NULL DEFAULT 1 CHECK(fingerprint_version=1),fingerprint TEXT NOT NULL,created_at TEXT NOT NULL,UNIQUE(source_schema_version,target_schema_version,operation,scope_id),UNIQUE(fingerprint_version,fingerprint));")?;
    Ok(())
}

#[cfg(test)]
#[cfg_attr(test, allow(dead_code))]
pub fn canonical_v2_connection() -> Result<Connection, Error> {
    let mut connection = Connection::open_in_memory()?;
    connection.execute_batch("PRAGMA foreign_keys=ON")?;
    let tx = connection.transaction()?;
    create_schema(&tx)?;
    prepare_canonical_v2(&tx)?;
    tx.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )?;
    tx.commit()?;
    Ok(connection)
}

#[cfg(test)]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn canonical_v1_connection() -> Result<Connection, Error> {
    let mut connection = Connection::open_in_memory()?;
    connection.execute_batch("PRAGMA foreign_keys=ON")?;
    let tx = connection.transaction()?;
    create_schema(&tx)?;
    tx.execute_batch("DROP TABLE compatibility_decisions;")?;
    tx.commit()?;
    Ok(connection)
}

#[cfg(test)]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn canonical_descriptor(
    db: &Connection,
) -> Result<Vec<(String, String, String)>, Error> {
    physical_descriptor(db)
}

#[cfg(test)]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn canonical_metadata(db: &Connection) -> Result<Vec<(String, i64)>, Error> {
    Ok(db
        .prepare("SELECT key,value FROM schema_metadata ORDER BY key")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?)
}

pub(super) fn ensure_slice4_v3_schema(db: &Transaction<'_>) -> Result<(), Error> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS reconciliation_runs(id INTEGER PRIMARY KEY, schema_version INTEGER NOT NULL DEFAULT 1, scope_id TEXT NOT NULL, run_sequence INTEGER NOT NULL, event_generation TEXT NOT NULL DEFAULT 'legacy', event_schema INTEGER NOT NULL DEFAULT 1, projection_generation TEXT NOT NULL DEFAULT 'legacy', projection_schema INTEGER NOT NULL DEFAULT 1, projection_authority INTEGER NOT NULL DEFAULT 0 CHECK(projection_authority IN(0,1)), filesystem_identity TEXT, filesystem_hash TEXT, git_state TEXT, status TEXT NOT NULL, repair_applied INTEGER NOT NULL DEFAULT 0 CHECK(repair_applied IN(0,1)), expected_head TEXT, observed_head TEXT, source_lineage TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE(scope_id,run_sequence), UNIQUE(id,scope_id)); CREATE TABLE IF NOT EXISTS reconciliation_discrepancies(id INTEGER PRIMARY KEY, run_id INTEGER NOT NULL, scope_id TEXT NOT NULL, source_lineage TEXT NOT NULL, correction_of_id INTEGER, correction_of_run_id INTEGER, provenance_kind TEXT, provenance_id TEXT, observation_sequence INTEGER NOT NULL DEFAULT 0, observed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, surface TEXT, expected TEXT, observed TEXT, status TEXT, reason TEXT, schema_version INTEGER NOT NULL DEFAULT 1, UNIQUE(id,run_id), UNIQUE(run_id,correction_of_id), FOREIGN KEY(run_id) REFERENCES reconciliation_runs(id));")?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS reconciliation_projection_evidence(run_id INTEGER NOT NULL, projection_id TEXT NOT NULL, scope TEXT NOT NULL, projection_schema INTEGER NOT NULL, event_generation INTEGER NOT NULL, source_generation INTEGER NOT NULL, status TEXT NOT NULL, authority INTEGER NOT NULL, rebuild_status TEXT NOT NULL, rebuild_authority INTEGER NOT NULL, generation_match INTEGER NOT NULL, schema_lineage TEXT NOT NULL, PRIMARY KEY(run_id,projection_id), FOREIGN KEY(run_id) REFERENCES reconciliation_runs(id)); CREATE INDEX IF NOT EXISTS reconciliation_projection_evidence_run ON reconciliation_projection_evidence(run_id);")?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS recovery_actions(action_id INTEGER PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),operation_id TEXT NOT NULL UNIQUE,run_id INTEGER NOT NULL,discrepancy_id INTEGER NOT NULL,scope_id TEXT NOT NULL,fingerprint_version INTEGER NOT NULL DEFAULT 1 CHECK(fingerprint_version=1),fingerprint TEXT NOT NULL,target_identity TEXT NOT NULL,expected_pre_hash TEXT,expected_pre_identity TEXT,intended_post_hash TEXT,intended_post_identity TEXT,authorization_identity TEXT NOT NULL,authorization_scope TEXT NOT NULL,stage TEXT NOT NULL CHECK(stage='prepared'),sequence INTEGER NOT NULL CHECK(sequence=0),source_lineage TEXT NOT NULL,UNIQUE(action_id,run_id),UNIQUE(fingerprint_version,fingerprint),FOREIGN KEY(run_id) REFERENCES reconciliation_runs(id),FOREIGN KEY(discrepancy_id,run_id) REFERENCES reconciliation_discrepancies(id,run_id)); CREATE TABLE IF NOT EXISTS recovery_outcomes(outcome_id INTEGER PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),action_id INTEGER NOT NULL,run_id INTEGER NOT NULL,scope_id TEXT NOT NULL,sequence INTEGER NOT NULL CHECK(sequence>=1),status TEXT NOT NULL,detail TEXT CHECK(detail IS NULL),source_lineage TEXT NOT NULL,stage TEXT NOT NULL,result TEXT NOT NULL,observed_pre_state TEXT,observed_post_state TEXT,provenance_kind TEXT NOT NULL,provenance_id TEXT NOT NULL,observation_sequence INTEGER NOT NULL CHECK(observation_sequence>=0),observed_at TEXT NOT NULL,supersedes_id INTEGER,supersedes_action_id INTEGER,UNIQUE(outcome_id,action_id),UNIQUE(action_id,sequence),UNIQUE(action_id,supersedes_id),FOREIGN KEY(action_id,run_id) REFERENCES recovery_actions(action_id,run_id),FOREIGN KEY(supersedes_id,supersedes_action_id) REFERENCES recovery_outcomes(outcome_id,action_id),CHECK(status=result),CHECK((supersedes_id IS NULL)=(supersedes_action_id IS NULL)));")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS reconciliation_runs_scope_sequence ON reconciliation_runs(scope_id,run_sequence); CREATE INDEX IF NOT EXISTS reconciliation_discrepancies_run_correction ON reconciliation_discrepancies(run_id,correction_of_id); CREATE INDEX IF NOT EXISTS recovery_actions_fingerprint ON recovery_actions(fingerprint_version,fingerprint); CREATE INDEX IF NOT EXISTS recovery_outcomes_action_sequence ON recovery_outcomes(action_id,sequence);")?;
    for table in [
        "reconciliation_runs",
        "reconciliation_discrepancies",
        "recovery_actions",
        "recovery_outcomes",
    ] {
        let trigger_prefix = match table {
            "reconciliation_runs"
            | "reconciliation_discrepancies"
            | "recovery_actions"
            | "recovery_outcomes" => table,
            _ => unreachable!(),
        };
        db.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS {trigger_prefix}_immutable_update BEFORE UPDATE ON {table} BEGIN SELECT RAISE(ABORT,'immutable {table}'); END; CREATE TRIGGER IF NOT EXISTS {trigger_prefix}_immutable_delete BEFORE DELETE ON {table} BEGIN SELECT RAISE(ABORT,'immutable {table}'); END;"))?;
    }
    db.execute_batch(r#"
CREATE TRIGGER IF NOT EXISTS versions_observation_guard BEFORE INSERT ON versions WHEN NEW.schema_version=3 AND NOT (
 (NEW.logical_path IS NULL AND NEW.observed_identity IS NULL AND NEW.git_applicability IS NULL AND NEW.git_repository_path IS NULL AND NEW.git_head_state IS NULL AND NEW.git_head_oid IS NULL AND NEW.git_index_present IS NULL AND NEW.git_index_fingerprint IS NULL)
 OR
 (length(NEW.logical_path)>0 AND length(NEW.observed_identity)>0 AND NEW.git_applicability='not_applicable' AND NEW.git_repository_path IS NULL AND NEW.git_head_state IS NULL AND NEW.git_head_oid IS NULL AND NEW.git_index_present IS NULL AND NEW.git_index_fingerprint IS NULL)
 ) BEGIN SELECT RAISE(ABORT,'incoherent versions observation'); END;
CREATE TRIGGER IF NOT EXISTS reconciliation_discrepancies_guards BEFORE INSERT ON reconciliation_discrepancies BEGIN
 SELECT CASE WHEN (NEW.correction_of_id IS NULL) != (NEW.correction_of_run_id IS NULL) THEN RAISE(ABORT,'correction parent pair') END;
 SELECT CASE WHEN NEW.correction_of_id IS NOT NULL AND (NEW.correction_of_id=NEW.id OR NEW.correction_of_run_id!=NEW.run_id OR NOT EXISTS(SELECT 1 FROM reconciliation_discrepancies WHERE id=NEW.correction_of_id AND run_id=NEW.correction_of_run_id AND scope_id=NEW.scope_id AND source_lineage=NEW.source_lineage) OR EXISTS(SELECT 1 FROM reconciliation_discrepancies WHERE correction_of_id=NEW.correction_of_id)) THEN RAISE(ABORT,'invalid correction') END;
END;
CREATE TRIGGER IF NOT EXISTS recovery_actions_guards BEFORE INSERT ON recovery_actions BEGIN
 SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM reconciliation_discrepancies WHERE id=NEW.discrepancy_id AND run_id=NEW.run_id AND scope_id=NEW.scope_id AND source_lineage=NEW.source_lineage) THEN RAISE(ABORT,'action discrepancy mismatch') END;
END;
CREATE TRIGGER IF NOT EXISTS recovery_outcomes_guards BEFORE INSERT ON recovery_outcomes BEGIN
 SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM recovery_actions WHERE action_id=NEW.action_id AND run_id=NEW.run_id AND scope_id=NEW.scope_id AND source_lineage=NEW.source_lineage) THEN RAISE(ABORT,'outcome action mismatch') END;
 SELECT CASE WHEN NEW.sequence=1 AND (NEW.stage!='effect_observed' OR NEW.result NOT IN ('effect_applied','effect_not_started','effect_unknown') OR NEW.supersedes_id IS NOT NULL) THEN RAISE(ABORT,'invalid effect') END;
 SELECT CASE WHEN NEW.sequence=1 AND ((NEW.result='effect_applied' AND (NEW.observed_pre_state IS NULL OR NEW.observed_post_state IS NULL)) OR (NEW.result='effect_not_started' AND (NEW.observed_pre_state IS NULL OR NEW.observed_post_state IS NOT NULL)) OR (NEW.result='effect_unknown' AND NEW.observed_post_state IS NULL)) THEN RAISE(ABORT,'invalid observations') END;
 SELECT CASE WHEN NEW.sequence=2 AND (NEW.stage!='terminal' OR NEW.result NOT IN ('applied','blocked','unknown','same') OR NOT EXISTS(SELECT 1 FROM recovery_outcomes WHERE outcome_id=NEW.supersedes_id AND action_id=NEW.supersedes_action_id AND action_id=NEW.action_id AND sequence=1 AND stage='effect_observed')) THEN RAISE(ABORT,'invalid terminal') END;
 SELECT CASE WHEN NEW.sequence>2 AND (NEW.stage!='correction' OR NEW.result NOT IN ('applied','blocked','unknown','same') OR NEW.provenance_kind!='reconciliation_discrepancy' OR NOT EXISTS(SELECT 1 FROM recovery_outcomes WHERE outcome_id=NEW.supersedes_id AND action_id=NEW.supersedes_action_id AND action_id=NEW.action_id AND sequence=NEW.sequence-1) OR NOT EXISTS(SELECT 1 FROM reconciliation_discrepancies d JOIN recovery_actions a ON a.action_id=NEW.action_id WHERE d.id=NEW.provenance_id AND d.run_id=NEW.run_id AND d.scope_id=NEW.scope_id AND d.source_lineage=NEW.source_lineage AND d.correction_of_id=CASE WHEN NEW.sequence=3 THEN a.discrepancy_id ELSE (SELECT provenance_id FROM recovery_outcomes WHERE action_id=NEW.action_id AND sequence=NEW.sequence-1) END)) THEN RAISE(ABORT,'invalid correction outcome') END;
 SELECT CASE WHEN NEW.sequence=2 AND ((NEW.result='applied' AND (SELECT result FROM recovery_outcomes WHERE outcome_id=NEW.supersedes_id AND action_id=NEW.supersedes_action_id)!='effect_applied') OR (NEW.result IN ('blocked','same') AND (SELECT result FROM recovery_outcomes WHERE outcome_id=NEW.supersedes_id AND action_id=NEW.supersedes_action_id)!='effect_not_started') OR (NEW.result='unknown' AND (SELECT result FROM recovery_outcomes WHERE outcome_id=NEW.supersedes_id AND action_id=NEW.supersedes_action_id)!='effect_unknown')) THEN RAISE(ABORT,'wrong terminal mapping') END;
 SELECT CASE WHEN NEW.sequence>1 AND NOT EXISTS(SELECT 1 FROM recovery_outcomes WHERE action_id=NEW.action_id AND sequence=NEW.sequence-1) THEN RAISE(ABORT,'sequence gap') END;
END;
"#)?;
    Ok(())
}

pub(super) fn physical_descriptor(db: &Connection) -> Result<Vec<(String, String, String)>, Error> {
    let mut statement = db.prepare(
        "SELECT type,name,coalesce(sql,'') FROM sqlite_master
         WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
    )?;
    let rows = statement
        .query_map([], |row| {
            let sql: String = row.get(2)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                sql.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

pub(super) fn validate_canonical_physical_schema(
    db: &Transaction<'_>,
    version: i64,
) -> Result<(), Error> {
    let mut reference = Connection::open_in_memory()?;
    reference.execute_batch("PRAGMA foreign_keys=ON")?;
    let tx = reference.transaction()?;
    create_schema(&tx)?;
    if version == 2 {
        prepare_canonical_v2(&tx)?;
    } else if version == 3 {
        prepare_canonical_v2(&tx)?;
        install_version_observation_columns(&tx)?;
        tx.execute_batch("DROP TABLE compatibility_decisions;")?;
        create_compatibility_v3(&tx)?;
        ensure_slice4_v3_schema(&tx)?;
    } else {
        return Err(Error::AmbiguousSchema);
    }
    tx.commit()?;
    let actual = physical_descriptor(db)?;
    let expected = physical_descriptor(&reference)?;
    if actual != expected {
        eprintln!("DIAGNOSTIC physical schema version={version}");
        for object in actual.iter().filter(|object| !expected.contains(object)) {
            eprintln!("DIAGNOSTIC actual-only: {object:?}");
        }
        for object in expected.iter().filter(|object| !actual.contains(object)) {
            eprintln!("DIAGNOSTIC reference-only: {object:?}");
        }
        for (actual_type, actual_name, actual_sql) in &actual {
            if let Some((_, _, expected_sql)) =
                expected.iter().find(|(expected_type, expected_name, _)| {
                    expected_type == actual_type && expected_name == actual_name
                })
            {
                if actual_sql != expected_sql {
                    eprintln!("DIAGNOSTIC same-name SQL difference {actual_type}/{actual_name}: actual={actual_sql:?} reference={expected_sql:?}");
                }
            }
        }
        return Err(Error::AmbiguousSchema);
    }
    eprintln!("DIAGNOSTIC physical comparison passed; next predicate: validate_slice4_v3_schema table reconciliation_runs existence");
    Ok(())
}

pub(super) fn validate_slice4_v3_schema(db: &Transaction<'_>) -> Result<(), Error> {
    // The physical descriptor above is authoritative.  Keep this small check
    // only for the objects whose presence is part of the Slice-4 boundary.
    for table in [
        "reconciliation_runs",
        "reconciliation_discrepancies",
        "recovery_actions",
        "recovery_outcomes",
    ] {
        if !table_exists(db, table)? {
            eprintln!(
                "DIAGNOSTIC first post-physical predicate failed: required table {table} missing"
            );
            return Err(Error::AmbiguousSchema);
        }
    }
    for index in [
        "reconciliation_runs_scope_sequence",
        "reconciliation_discrepancies_run_correction",
        "recovery_actions_fingerprint",
        "recovery_outcomes_action_sequence",
    ] {
        if !index_exists(db, index)? {
            eprintln!("DIAGNOSTIC post-physical predicate failed: required index {index} missing");
            return Err(Error::AmbiguousSchema);
        }
    }
    for table in [
        "reconciliation_runs",
        "reconciliation_discrepancies",
        "recovery_actions",
        "recovery_outcomes",
    ] {
        for suffix in ["update", "delete"] {
            if !trigger_exists(db, &format!("{table}_immutable_{suffix}"))? {
                eprintln!("DIAGNOSTIC post-physical predicate failed: required trigger {table}_immutable_{suffix} missing");
                return Err(Error::AmbiguousSchema);
            }
        }
    }
    for (table, required) in [
        (
            "reconciliation_runs",
            &[
                "event_generation",
                "event_schema",
                "projection_generation",
                "projection_schema",
                "projection_authority",
                "filesystem_identity",
                "filesystem_hash",
                "git_state",
                "repair_applied",
                "schema_version",
            ] as &[&str],
        ),
        (
            "reconciliation_discrepancies",
            &[
                "surface",
                "expected",
                "observed",
                "status",
                "reason",
                "schema_version",
            ],
        ),
    ] {
        let mut columns = std::collections::HashMap::new();
        let mut statement = db.prepare(&format!("PRAGMA table_info({table})"))?;
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, Option<String>>(4)?))
        })? {
            let (name, default) = row?;
            columns.insert(name, default);
        }
        if required.iter().any(|name| !columns.contains_key(*name))
            || (table == "reconciliation_runs" && columns["schema_version"].as_deref() != Some("1"))
        {
            eprintln!("DIAGNOSTIC post-physical predicate failed: required columns/defaults for {table}; actual={columns:?}");
            return Err(Error::AmbiguousSchema);
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub(super) fn validate_slice3_guards(db: &Transaction<'_>) -> Result<(), Error> {
    for trigger in [
        "immutable_projections_insert",
        "immutable_projections_update",
        "immutable_projection_rebuild_status_insert",
        "immutable_projection_rebuild_status_update",
    ] {
        if !trigger_exists(db, trigger)? {
            return Err(Error::AmbiguousSchema);
        }
    }
    let mut previous = None;
    for sequence in db
        .prepare("SELECT event_sequence FROM events WHERE schema_version=1 ORDER BY rowid")?
        .query_map([], |row| row.get::<_, i64>(0))?
    {
        let sequence = sequence?;
        if previous.is_some_and(|previous| sequence <= previous) {
            return Err(Error::AmbiguousSchema);
        }
        previous = Some(sequence);
    }
    for (insert, update, delete) in [
        ("INSERT INTO projection_rebuilds(projection_id,event_id,event_sequence,status,schema_version,source_lineage) VALUES('guard','event',1,'complete',1,'native')", "UPDATE projection_rebuilds SET status='failed' WHERE projection_id='guard'", "DELETE FROM projection_rebuilds WHERE projection_id='guard'"),
        ("INSERT INTO playback_results(id,event_history_generation,result,exact,schema_version,source_lineage) VALUES('guard',1,'exact_playback_succeeded',1,1,'native')", "UPDATE playback_results SET exact=0 WHERE id='guard'", "DELETE FROM playback_results WHERE id='guard'"),
        ("INSERT INTO captured_outcome_simulations(id,label,outcome,exact_playback_evidence,schema_version,source_lineage) VALUES('guard','captured_outcome_simulation','ok',0,1,'native')", "UPDATE captured_outcome_simulations SET outcome='bad' WHERE id='guard'", "DELETE FROM captured_outcome_simulations WHERE id='guard'"),
    ] {
        db.execute_batch("SAVEPOINT schema_guard")?;
        let valid = db.execute(insert, []).is_ok() && db.execute(update, []).is_err() && db.execute(delete, []).is_err();
        db.execute_batch("ROLLBACK TO schema_guard; RELEASE schema_guard")?;
        if !valid { return Err(Error::AmbiguousSchema); }
    }
    for (table, key) in [
        ("projections", "id"),
        ("projection_rebuild_status", "projection_id"),
    ] {
        db.execute_batch("SAVEPOINT authority_guard")?;
        if table == "projections" {
            db.execute("INSERT INTO projections(id,projection_schema_version,event_history_generation,source_generation,authoritative,status) VALUES('authority-guard',1,0,0,1,'complete')", [])?;
        } else {
            db.execute("INSERT INTO projection_rebuild_status(projection_id,status,authoritative) VALUES('authority-guard','complete',1)", [])?;
        }
        let insert = if table == "projections" {
            db.execute("INSERT INTO projections(id,projection_schema_version,event_history_generation,source_generation,authoritative,status) VALUES('authority-guard-bad',1,0,0,1,'stale')", [])
        } else {
            db.execute("INSERT INTO projection_rebuild_status(projection_id,status,authoritative) VALUES('authority-guard-bad','stale',1)", [])
        };
        let update = if table == "projections" {
            db.execute(
                "UPDATE projections SET status='failed',authoritative=1 WHERE id='authority-guard'",
                [],
            )
        } else {
            db.execute("UPDATE projection_rebuild_status SET status='failed',authoritative=1 WHERE projection_id='authority-guard'", [])
        };
        let valid = insert.is_err() && update.is_err();
        let _ = key;
        db.execute_batch("ROLLBACK TO authority_guard; RELEASE authority_guard")?;
        if !valid {
            return Err(Error::AmbiguousSchema);
        }
    }
    for (n, result, exact, divergence, accepted) in [
        (0, "exact_playback_succeeded", 1, None, true),
        (1, "exact_playback_succeeded", 0, None, false),
        (2, "exact_playback_succeeded", 1, Some("drift"), false),
        (3, "diverged", 0, Some("drift"), true),
        (4, "diverged", 1, Some("drift"), false),
        (5, "diverged", 0, None, false),
    ] {
        db.execute_batch("SAVEPOINT schema_guard")?;
        let inserted = db.execute(
            "INSERT INTO playback_results(id,event_history_generation,result,divergence,exact,schema_version,source_lineage) VALUES(?,1,?,?,?,1,'native')",
            rusqlite::params![format!("guard-coherence-{n}"), result, divergence, exact],
        ).is_ok();
        db.execute_batch("ROLLBACK TO schema_guard; RELEASE schema_guard")?;
        if inserted != accepted {
            return Err(Error::AmbiguousSchema);
        }
    }
    db.execute_batch("SAVEPOINT schema_guard; DROP TRIGGER immutable_playback_results_update")?;
    let mut updates_ok = true;
    for (n, result, exact, divergence, accepted) in [
        (0, "exact_playback_succeeded", 1, None, true),
        (1, "exact_playback_succeeded", 0, None, false),
        (2, "exact_playback_succeeded", 1, Some("drift"), false),
        (3, "diverged", 0, Some("drift"), true),
        (4, "diverged", 1, Some("drift"), false),
        (5, "diverged", 0, None, false),
    ] {
        let id = format!("guard-update-{n}");
        db.execute("INSERT INTO playback_results(id,event_history_generation,result,exact,schema_version,source_lineage) VALUES(?,1,'exact_playback_succeeded',1,1,'native')", [&id])?;
        let changed = db
            .execute(
                "UPDATE playback_results SET result=?,divergence=?,exact=? WHERE id=?",
                rusqlite::params![result, divergence, exact, id],
            )
            .is_ok();
        updates_ok &= changed == accepted;
    }
    db.execute_batch("ROLLBACK TO schema_guard; RELEASE schema_guard")?;
    if !updates_ok {
        return Err(Error::AmbiguousSchema);
    }
    Ok(())
}

pub(super) fn ensure_compatibility_decision_table(db: &Transaction<'_>) -> Result<(), Error> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS compatibility_decisions(id INTEGER PRIMARY KEY,source_schema_version INTEGER NOT NULL,target_schema_version INTEGER NOT NULL,operation TEXT NOT NULL,actor TEXT NOT NULL,source TEXT NOT NULL,outcome TEXT NOT NULL,fingerprint TEXT NOT NULL UNIQUE,result TEXT NOT NULL)").map_err(Error::Sql)
}

pub(super) fn require_slice4_decision(db: &Transaction<'_>) -> Result<(), Error> {
    let count: i64 = db.query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version=2 AND target_schema_version=3", [], |r| r.get(0))?;
    if count != 1 {
        return Err(Error::AmbiguousSchema);
    }
    Ok(())
}

pub(super) fn columns(db: &Transaction<'_>, table: &str) -> Result<Vec<String>, Error> {
    Ok(db
        .prepare(&format!(
            "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
        ))?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?)
}

pub(super) fn validate_v2_compatibility(
    db: &Transaction<'_>,
    context: ValidationContext,
) -> Result<(), Error> {
    validate_canonical_physical_schema(db, 2)?;
    let metadata: i64 = db.query_row(
        "SELECT count(*) FROM schema_metadata WHERE (key='schema_version' AND value=?) OR (key='migration_lineage' AND value=1)",
        [context.expected_metadata().0],
        |r| r.get(0),
    )?;
    if metadata != 2 {
        return Err(Error::AmbiguousSchema);
    }
    for table in [
        "reconciliation_runs",
        "reconciliation_discrepancies",
        "recovery_actions",
        "recovery_outcomes",
    ] {
        if table_exists(db, table)? {
            return Err(Error::AmbiguousSchema);
        }
    }
    if columns(db, "events")?
        != [
            "id",
            "schema_version",
            "source_lineage",
            "correction_of",
            "event_sequence",
            "transition",
            "deterministic_input",
            "outcome",
        ]
    {
        return Err(Error::AmbiguousSchema);
    }
    if columns(db, "compatibility_decisions")?
        != [
            "id",
            "source_schema_version",
            "target_schema_version",
            "operation",
            "actor",
            "source",
            "outcome",
            "fingerprint",
            "result",
        ]
    {
        return Err(Error::AmbiguousSchema);
    }
    require_slice3_decision(db)?;
    // Reuse the fail-closed Slice-3 validators: migration must not repair or
    // manufacture historical schema guards before this checkpoint passes.
    validate_runtime_record_schema(db)?;
    validate_slice3_guards(db)
}

pub(super) fn validate_v3_compatibility(db: &Transaction<'_>) -> Result<(), Error> {
    if columns(db, "compatibility_decisions")?
        != [
            "id",
            "source_schema_version",
            "target_schema_version",
            "operation",
            "actor",
            "source",
            "outcome",
            "result",
            "legacy_fingerprint",
            "scope_id",
            "fingerprint_version",
            "fingerprint",
            "created_at",
        ]
    {
        return Err(Error::AmbiguousSchema);
    }
    let mut rows = db.prepare("SELECT id,source_schema_version,target_schema_version,operation,actor,source,outcome,result,legacy_fingerprint,scope_id,fingerprint_version,fingerprint,created_at FROM compatibility_decisions ORDER BY id")?;
    let decisions = rows
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, i64>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, String>(12)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut transitions = Vec::new();
    for (
        _id,
        source,
        target,
        operation,
        actor,
        origin,
        outcome,
        result,
        legacy,
        scope,
        version,
        fingerprint,
        created_at,
    ) in decisions
    {
        if created_at.is_empty()
            || actor != "system"
            || version != 1
            || scope != "store"
            || fingerprint
                != decision_fingerprint(
                    source, target, &operation, &actor, &origin, &outcome, &result, "store",
                )
        {
            return Err(Error::AmbiguousSchema);
        }
        let expected = match (source, target) {
            (0, 1) => ("migration", "artifact-store", "accepted", "migrated"),
            (1, 2) => (
                "slice-3-additive-compatibility",
                "artifact-store",
                "accepted",
                "migrated",
            ),
            (2, 3) => (
                "slice-4-additive-compatibility",
                "store",
                "migrated",
                "migrated",
            ),
            _ => return Err(Error::AmbiguousSchema),
        };
        let expected_legacy = match (source, target) {
            (0, 1) => Some("legacy-0-to-1"),
            (1, 2) => Some("slice-3-additive-compatibility"),
            (2, 3) => None,
            _ => return Err(Error::AmbiguousSchema),
        };
        if (
            operation.as_str(),
            origin.as_str(),
            outcome.as_str(),
            result.as_str(),
        ) != expected
            || legacy.as_deref() != expected_legacy
        {
            return Err(Error::AmbiguousSchema);
        }
        transitions.push((source, target));
    }
    if !matches!(
        transitions.as_slice(),
        [(2, 3)] | [(1, 2), (2, 3)] | [(0, 1), (1, 2), (2, 3)]
    ) {
        return Err(Error::AmbiguousSchema);
    }
    Ok(())
}

pub(super) fn validate_sqlite_integrity(db: &Transaction<'_>) -> Result<(), Error> {
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(Error::AmbiguousSchema);
    }
    if db
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |_| Ok(()))?
        .count()
        != 0
    {
        return Err(Error::AmbiguousSchema);
    }
    Ok(())
}
#[allow(dead_code)]
pub(super) fn require_slice3_decision(db: &Transaction<'_>) -> Result<(), Error> {
    let count: i64 = db.query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version=1 AND target_schema_version=2", [], |r| r.get(0))?;
    let canonical: i64 = db.query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version=1 AND target_schema_version=2 AND operation='slice-3-additive-compatibility' AND actor='system' AND source='artifact-store' AND outcome='accepted' AND fingerprint='slice-3-additive-compatibility' AND result='migrated'", [], |r| r.get(0))?;
    if count != 1 || canonical != 1 {
        return Err(Error::AmbiguousSchema);
    }
    Ok(())
}
