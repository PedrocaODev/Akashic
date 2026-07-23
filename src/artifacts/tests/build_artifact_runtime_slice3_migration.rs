use crate::artifacts;
use artifacts::{canonical_descriptor, canonical_metadata, canonical_v1_connection, Error, Store};
use rusqlite::Connection;

#[test]
fn v1_fixture_matches_canonical_baseline_before_migration() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/slice2_v1.sql"))
        .unwrap();
    let canonical = canonical_v1_connection().unwrap();
    assert_eq!(
        canonical_descriptor(&db).unwrap(),
        canonical_descriptor(&canonical).unwrap()
    );
    assert_eq!(
        canonical_metadata(&db).unwrap(),
        canonical_metadata(&canonical).unwrap()
    );
    assert_eq!(
        db.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT source_lineage FROM versions WHERE id='v'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "native"
    );
    assert_eq!(
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    for table in [
        "projections",
        "projection_rebuilds",
        "projection_rebuild_status",
        "playback_results",
        "captured_outcome_simulations",
        "reconciliation_runs",
        "reconciliation_discrepancies",
        "recovery_actions",
        "recovery_outcomes",
    ] {
        assert!(
            !db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
            "unexpected later-version object: {table}"
        );
    }
}

#[test]
fn frozen_schema_version_one_fixture_migrates_rows_and_reopens_idempotently() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/slice2_v1.sql"))
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    for table in [
        "projections",
        "projection_rebuilds",
        "projection_rebuild_status",
        "playback_results",
        "captured_outcome_simulations",
    ] {
        assert!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?",
                [table],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
                == 0,
            "unexpected Slice 3 table: {table}"
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='compatibility_decisions'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT operation,operation_result_id FROM discrepancies WHERE id=1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        ("op".to_string(), "v".to_string())
    );
    assert_eq!(
        db.query_row(
            "SELECT source_schema_version,source_lineage FROM versions WHERE id='v'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        (1, "native".to_string())
    );
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let store = Store::from_connection(db).unwrap();
    for table in [
        "artifacts",
        "versions",
        "operations",
        "lineage",
        "discrepancies",
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
        assert_eq!(
            store
                .connection()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            if matches!(
                table,
                "artifacts" | "versions" | "operations" | "lineage" | "discrepancies"
            ) {
                1
            } else {
                0
            },
            "{table}"
        );
    }
    assert_eq!(
        store
            .connection()
            .query_row("SELECT canonical FROM versions WHERE id='v'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "content"
    );
    assert_eq!(store.schema_version().unwrap(), 3);
    assert_eq!(
        store
            .connection()
            .query_row(
                "SELECT schema_version FROM artifacts WHERE id='a'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .connection()
            .query_row(
                "SELECT source_schema_version,source_lineage FROM versions WHERE id='v'",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            )
            .unwrap(),
        (1, "native".to_string())
    );
    assert_eq!(
        store
            .connection()
            .query_row(
                "SELECT operation,operation_result_id FROM discrepancies WHERE id=1",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            )
            .unwrap(),
        ("op".to_string(), "v".to_string())
    );
    assert_eq!(store.connection().query_row("SELECT operation,actor,source,outcome,legacy_fingerprint,result FROM compatibility_decisions WHERE source_schema_version=1", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))).unwrap(), ("slice-3-additive-compatibility".to_string(), "system".to_string(), "artifact-store".to_string(), "accepted".to_string(), "slice-3-additive-compatibility".to_string(), "migrated".to_string()));
    assert_eq!(store.connection().query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version=1 AND target_schema_version=2 AND operation='slice-3-additive-compatibility'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.connection().query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version=1 AND target_schema_version=2", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_partial_slice_three_schema_is_rejected_without_reconstruction() {
    let path =
        std::env::temp_dir().join(format!("akashic-slice3-partial-{}.db", std::process::id()));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute("DROP TABLE projections", []).unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_requires_one_exact_canonical_decision() {
    let path =
        std::env::temp_dir().join(format!("akashic-slice3-decision-{}.db", std::process::id()));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute("DELETE FROM compatibility_decisions", [])
        .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_missing_slice_two_record_table_is_ambiguous() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-missing-record-{}.db",
        std::process::id()
    ));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute("DROP TABLE findings", []).unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_malformed_non_core_record_table_is_ambiguous() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-malformed-record-{}.db",
        std::process::id()
    ));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute("ALTER TABLE findings ADD COLUMN unexpected TEXT", [])
        .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_malformed_playback_coherence_guard_is_ambiguous() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-playback-guard-{}.db",
        std::process::id()
    ));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute(
        "DROP TRIGGER immutable_playback_results_coherence_insert",
        [],
    )
    .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_missing_projection_authority_guard_is_ambiguous() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-missing-authority-{}.db",
        std::process::id()
    ));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute("DROP TRIGGER immutable_projections_insert", [])
        .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_noop_append_only_guard_is_ambiguous() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-noop-guard-{}.db",
        std::process::id()
    ));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute_batch("DROP TRIGGER immutable_findings_update; CREATE TRIGGER immutable_findings_update BEFORE UPDATE ON findings BEGIN SELECT 1; END;").unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_append_only_and_playback_guards_are_behaviorally_verified() {
    let path =
        std::env::temp_dir().join(format!("akashic-slice3-behavior-{}.db", std::process::id()));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO projection_rebuilds(projection_id,event_id,event_sequence,status) VALUES('p','e',1,'complete')", []).unwrap();
    db.execute("INSERT INTO playback_results(id,event_history_generation,result,exact) VALUES('r',1,'exact_playback_succeeded',1)", []).unwrap();
    db.execute("INSERT INTO captured_outcome_simulations(id,label,outcome) VALUES('s','captured_outcome_simulation','ok')", []).unwrap();
    db.execute_batch("DROP TRIGGER immutable_projection_rebuilds_update; CREATE TRIGGER immutable_projection_rebuilds_update BEFORE UPDATE ON projection_rebuilds BEGIN SELECT 1; END;").unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_two_slice_three_defaults_must_be_canonical() {
    let path =
        std::env::temp_dir().join(format!("akashic-slice3-default-{}.db", std::process::id()));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=2 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    db.execute_batch("DROP TABLE captured_outcome_simulations; CREATE TABLE captured_outcome_simulations(id TEXT PRIMARY KEY,label TEXT NOT NULL,outcome TEXT NOT NULL,exact_playback_evidence INTEGER NOT NULL DEFAULT 1 CHECK(exact_playback_evidence=0),schema_version INTEGER NOT NULL DEFAULT 9,source_lineage TEXT NOT NULL DEFAULT 'wrong');").unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn playback_coherence_guards_fail_closed_for_insert_and_update() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-coherence-{}.db",
        std::process::id()
    ));
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER immutable_playback_results_update")
        .unwrap();
    let cases = [
        ("exact_playback_succeeded", 1, None, true),
        ("exact_playback_succeeded", 1, Some("diverged"), false),
        ("exact_playback_succeeded", 0, None, false),
        ("diverged", 0, Some("diverged"), true),
        ("diverged", 1, Some("diverged"), false),
        ("diverged", 0, None, false),
    ];
    for (n, (result, exact, divergence, allowed)) in cases.iter().enumerate() {
        db.execute_batch("SAVEPOINT coherence").unwrap();
        let insert = db.execute("INSERT INTO playback_results(id,event_history_generation,result,divergence,exact) VALUES(?,?,?,?,?)", rusqlite::params![format!("insert-{n}"), 1, result, divergence, exact]);
        assert_eq!(insert.is_ok(), *allowed, "insert case {n}");
        db.execute_batch("ROLLBACK TO coherence; RELEASE coherence")
            .unwrap();
        let target = format!("update-target-{n}");
        db.execute("INSERT INTO playback_results(id,event_history_generation,result,divergence,exact) VALUES(?,1,'exact_playback_succeeded',NULL,1)", [&target]).unwrap();
        db.execute_batch("SAVEPOINT coherence").unwrap();
        let update = db.execute(
            "UPDATE playback_results SET result=?, divergence=?, exact=? WHERE id=?",
            rusqlite::params![result, divergence, exact, target],
        );
        assert_eq!(update.is_ok(), *allowed, "update case {n}");
        db.execute_batch("ROLLBACK TO coherence; RELEASE coherence")
            .unwrap();
    }
    std::fs::remove_file(path).unwrap();
}
