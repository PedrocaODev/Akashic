use crate::artifacts;
use artifacts::{canonical_descriptor, canonical_metadata, canonical_v2_connection, Error, Store};
use rusqlite::{types::Value, Connection};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;

const SLICE4: [&str; 4] = [
    "reconciliation_runs",
    "reconciliation_discrepancies",
    "recovery_actions",
    "recovery_outcomes",
];

fn object_exists(db: &Connection, kind: &str, name: &str) -> bool {
    db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type=?1 AND name=?2",
        (kind, name),
        |row| row.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

fn physical_snapshot(db: &Connection) -> Vec<String> {
    let mut out = Vec::new();
    let mut objects = db
        .prepare("SELECT type,name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name")
        .unwrap();
    let rows = objects
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .unwrap();
    for row in rows {
        let (kind, name, sql) = row.unwrap();
        out.push(format!("{kind}:{name}:{sql:?}"));
        if kind == "table" {
            let mut stmt = db
                .prepare(&format!("SELECT * FROM \"{name}\" ORDER BY rowid"))
                .unwrap();
            let columns = stmt.column_count();
            let values = stmt
                .query_map([], |r| {
                    (0..columns)
                        .map(|i| r.get::<_, Value>(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap();
            for value in values {
                out.push(format!("{name}:{:?}", value.unwrap()));
            }
        }
    }
    out
}

fn table_snapshot(db: &Connection, table: &str) -> Vec<Vec<Value>> {
    let mut stmt = db
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn v2_fixture() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice4-v2-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/slice3_v2.sql"))
        .unwrap();
    drop(db);
    path
}

fn fresh_path(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("akashic-slice4-{name}-{}", std::process::id()));
    let _ = fs::remove_file(&path);
    Store::open(&path).unwrap();
    path
}

#[test]
fn v2_fixture_matches_canonical_baseline_before_slice4_migration() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/slice3_v2.sql"))
        .unwrap();
    let canonical = canonical_v2_connection().unwrap();
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
        2
    );
    assert_eq!(
        db.query_row("SELECT source_lineage FROM events WHERE id='e'", [], |r| {
            r.get::<_, String>(0)
        })
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
    for table in SLICE4 {
        assert!(
            !object_exists(&db, "table", table),
            "unexpected Slice-4 object: {table}"
        );
    }
}

#[test]
fn fresh_v3_has_exact_slice4_schema_and_one_creation_decision() {
    let store = Store::open_in_memory().unwrap();
    let db = store.connection();
    for table in SLICE4 {
        assert!(object_exists(db, "table", table), "missing {table}");
    }
    for index in [
        "reconciliation_runs_scope_sequence",
        "reconciliation_discrepancies_run_correction",
        "recovery_actions_fingerprint",
        "recovery_outcomes_action_sequence",
    ] {
        assert!(object_exists(db, "index", index), "missing {index}");
    }
    assert_eq!(store.schema_version().unwrap(), 3);
    assert_eq!(db.query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version=2 AND target_schema_version=3 AND operation='slice-4-additive-compatibility'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn populated_v2_migrates_without_losing_rows_or_lineage_and_reopens_idempotently() {
    let path = v2_fixture();
    let before = physical_snapshot(&Connection::open(&path).unwrap());
    let before_rows: Vec<_> = [
        "schema_metadata",
        "artifacts",
        "versions",
        "operations",
        "lineage",
        "events",
        "compatibility_decisions",
    ]
    .iter()
    .map(|table| {
        (
            *table,
            table_snapshot(&Connection::open(&path).unwrap(), table),
        )
    })
    .collect();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 3);
    assert_eq!(
        store
            .connection()
            .query_row("SELECT count(*) FROM artifacts WHERE id='a'", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .connection()
            .query_row("SELECT source_lineage FROM events WHERE id='e'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "native"
    );
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.schema_version().unwrap(), 3);
    assert_eq!(
        reopened
            .connection()
            .query_row("SELECT count(*) FROM artifacts WHERE id='a'", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let after_db = reopened.connection();
    for (table, rows) in before_rows {
        let after = table_snapshot(after_db, table);
        if table == "schema_metadata" {
            assert_eq!(after.len(), rows.len());
        } else if table == "compatibility_decisions" {
            assert_eq!(&after[0][..7], &rows[0][..7]);
            assert_eq!(after[0][7], rows[0][8]);
            assert_eq!(after[0][8], rows[0][7]);
        } else {
            let expected = if table == "versions" {
                rows.into_iter()
                    .map(|mut row| {
                        row.extend((0..7).map(|_| Value::Null));
                        row
                    })
                    .collect()
            } else {
                rows
            };
            assert_eq!(after, expected, "{table}");
        }
    }
    assert!(!before.is_empty());
    assert_eq!(
        reopened
            .connection()
            .query_row("SELECT source_lineage FROM events WHERE id='e'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "native"
    );
    assert_eq!(reopened.connection().query_row("SELECT count(*) FROM compatibility_decisions WHERE source_schema_version=2 AND target_schema_version=3", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(
        reopened
            .connection()
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        reopened
            .connection()
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn populated_v2_preserves_every_durable_value_in_a_typed_ledger() {
    type Ledger = Vec<(String, Vec<Vec<Value>>)>;
    fn ledger(db: &Connection) -> Ledger {
        const TABLES: &[&str] = &[
            "schema_metadata",
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
            "projections",
            "projection_rebuilds",
            "projection_rebuild_status",
            "playback_results",
            "captured_outcome_simulations",
        ];
        TABLES.iter().map(|&table| (table.into(), table_snapshot(db, table))).chain(std::iter::once(("compatibility_decisions".into(), {
            let fp = if db.query_row("SELECT count(*) FROM pragma_table_info('compatibility_decisions') WHERE name='legacy_fingerprint'", [], |r| r.get::<_, i64>(0)).unwrap() == 1 { "legacy_fingerprint" } else { "fingerprint" };
            let sql = format!("SELECT id,source_schema_version,target_schema_version,operation,actor,source,outcome,result,{fp} FROM compatibility_decisions WHERE target_schema_version=2 ORDER BY id");
            db.prepare(&sql).unwrap().query_map([], |r| (0..9).map(|i| r.get(i)).collect()).unwrap().map(Result::unwrap).collect()
        }))).collect()
    }
    let path = v2_fixture();
    let before = ledger(&Connection::open(&path).unwrap());
    let store = Store::open(&path).unwrap();
    let after = ledger(store.connection());
    assert_eq!(
        store
            .connection()
            .query_row("SELECT count(*) FROM reconciliation_runs", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .connection()
            .query_row(
                "SELECT count(*) FROM reconciliation_discrepancies",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .connection()
            .query_row("SELECT count(*) FROM recovery_actions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .connection()
            .query_row("SELECT count(*) FROM recovery_outcomes", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let mut expected = before.clone();
    expected
        .iter_mut()
        .find(|(t, _)| t == "schema_metadata")
        .unwrap()
        .1
        .iter_mut()
        .for_each(|row| {
            if row[0] == Value::Text("schema_version".into()) {
                row[1] = Value::Integer(3);
            }
        });
    expected
        .iter_mut()
        .find(|(t, _)| t == "versions")
        .unwrap()
        .1
        .iter_mut()
        .for_each(|row| row.extend((0..7).map(|_| Value::Null)));
    assert_eq!(after, expected);
    fs::remove_file(path).unwrap();
}

#[test]
fn noncontiguous_historical_decision_ids_are_preserved_and_new_transition_is_last() {
    let path = v2_fixture();
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE compatibility_decisions SET id=41", [])
        .unwrap();
    drop(db);
    let store = Store::open(&path).unwrap();
    let ids: Vec<i64> = store
        .connection()
        .prepare("SELECT id FROM compatibility_decisions ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, vec![41, 42]);
    drop(store);
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .connection()
            .query_row(
                "SELECT group_concat(id, ',') FROM compatibility_decisions ORDER BY id",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "41,42"
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn canonical_v3_semantic_tamper_matrix_fails_closed() {
    let cases = [
        ("actor", "UPDATE compatibility_decisions SET actor='tampered' WHERE source_schema_version=2"),
        ("source", "UPDATE compatibility_decisions SET source='tampered' WHERE source_schema_version=2"),
        ("scope", "UPDATE compatibility_decisions SET scope_id='tampered' WHERE source_schema_version=2"),
        ("result", "UPDATE compatibility_decisions SET result='tampered' WHERE source_schema_version=2"),
        ("outcome", "UPDATE compatibility_decisions SET outcome='tampered' WHERE source_schema_version=2"),
        ("legacy_fingerprint", "UPDATE compatibility_decisions SET legacy_fingerprint='tampered' WHERE source_schema_version=2"),
        ("fingerprint", "UPDATE compatibility_decisions SET fingerprint='tampered' WHERE source_schema_version=2"),
    ];
    for (name, mutation) in cases {
        let path = fresh_path(name);
        let db = Connection::open(&path).unwrap();
        db.execute(mutation, []).unwrap();
        drop(db);
        assert!(
            matches!(Store::open(&path), Err(Error::AmbiguousSchema)),
            "{name}"
        );
        fs::remove_file(path).unwrap();
    }
    let path = fresh_path("fingerprint-version");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE compatibility_decisions SET fingerprint_version=9 WHERE source_schema_version=2;").unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    fs::remove_file(path).unwrap();
}

#[test]
fn compatibility_transition_shape_tampering_and_duplicate_are_rejected() {
    let path = fresh_path("transitions");
    let db = Connection::open(&path).unwrap();
    let fp = artifacts::test_decision_fingerprint(
        9, 10, "unknown", "system", "store", "migrated", "migrated", "store",
    );
    db.execute("INSERT INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,result,scope_id,fingerprint_version,fingerprint,created_at) VALUES(9,10,'unknown','system','store','migrated','migrated','store',1,?, 'now')", [&fp]).unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    fs::remove_file(path).unwrap();

    let path = fresh_path("duplicate");
    let db = Connection::open(&path).unwrap();
    let duplicate = db.execute("INSERT INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,result,scope_id,fingerprint_version,fingerprint,created_at) SELECT source_schema_version,target_schema_version,operation,actor,source,outcome,result,scope_id,fingerprint_version,fingerprint,created_at FROM compatibility_decisions WHERE source_schema_version=2", []).unwrap_err();
    assert!(duplicate.to_string().contains("UNIQUE"));
    fs::remove_file(path).unwrap();
}

#[test]
fn missing_middle_and_reversed_transition_ids_fail_closed() {
    let path = fresh_path("missing-middle");
    let db = Connection::open(&path).unwrap();
    db.execute(
        "DELETE FROM compatibility_decisions WHERE source_schema_version=2",
        [],
    )
    .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    fs::remove_file(path).unwrap();

    let path = fresh_path("reversed-ids");
    let db = Connection::open(&path).unwrap();
    db.execute("INSERT INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,result,scope_id,fingerprint_version,fingerprint,created_at) VALUES(1,2,'slice-3-additive-compatibility','system','artifact-store','accepted','migrated','store',1,?, 'now')", [artifacts::test_decision_fingerprint(1, 2, "slice-3-additive-compatibility", "system", "artifact-store", "accepted", "migrated", "store")]).unwrap();
    db.execute(
        "UPDATE compatibility_decisions SET id=99 WHERE source_schema_version=2",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE compatibility_decisions SET id=2 WHERE source_schema_version=1",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE compatibility_decisions SET id=1 WHERE source_schema_version=2",
        [],
    )
    .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    fs::remove_file(path).unwrap();
}

#[test]
fn created_at_is_nonempty_but_not_fingerprint_identity() {
    let path = fresh_path("created-at");
    let db = Connection::open(&path).unwrap();
    let old: String = db
        .query_row(
            "SELECT fingerprint FROM compatibility_decisions WHERE source_schema_version=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch(
        "UPDATE compatibility_decisions SET created_at='different' WHERE source_schema_version=2;",
    )
    .unwrap();
    drop(db);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.connection().query_row("SELECT fingerprint,created_at FROM compatibility_decisions WHERE source_schema_version=2", [], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).unwrap(), (old, "different".into()));
    fs::remove_file(path).unwrap();

    let path = fresh_path("empty-created-at");
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE compatibility_decisions SET created_at='' WHERE source_schema_version=2",
        [],
    )
    .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    fs::remove_file(path).unwrap();
}

#[test]
fn failpoint_rolls_back_actual_post_ddl_v2_migration_byte_for_byte() {
    let path = v2_fixture();
    let before = physical_snapshot(&Connection::open(&path).unwrap());
    let _failpoint = artifacts::fail_next_migration_checkpoint();
    assert!(matches!(Store::open(&path), Err(Error::RebuildFailure)));
    let db = Connection::open(&path).unwrap();
    assert_eq!(physical_snapshot(&db), before);
    assert_eq!(
        db.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    for object in SLICE4 {
        assert!(!object_exists(&db, "table", object));
    }
    assert!(!object_exists(&db, "table", "compatibility_decisions_v2"));
    fs::remove_file(path).unwrap();
}

#[test]
fn failpoint_rolls_back_actual_post_ddl_v2_and_v1_migrations() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice4-v1-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/slice2_v1.sql"))
        .unwrap();
    let before = physical_snapshot(&db);
    drop(db);
    let _failpoint = artifacts::fail_next_migration_checkpoint();
    assert!(matches!(Store::open(&path), Err(Error::RebuildFailure)));
    let db = Connection::open(&path).unwrap();
    assert_eq!(physical_snapshot(&db), before);
    assert_eq!(
        db.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn partial_slice4_malformed_data_guards_and_future_versions_fail_closed() {
    for mutation in [
        "CREATE TABLE reconciliation_runs(id INTEGER)",
        "ALTER TABLE events ADD COLUMN malformed INTEGER",
        "DROP TRIGGER immutable_events_update",
    ] {
        let db = Connection::open(v2_fixture()).unwrap();
        db.execute_batch(mutation).unwrap();
        assert!(matches!(
            Store::from_connection(db),
            Err(Error::AmbiguousSchema)
        ));
    }
    let db = Connection::open(v2_fixture()).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value=99 WHERE key='schema_version'",
        [],
    )
    .unwrap();
    assert!(matches!(
        Store::from_connection(db),
        Err(Error::UnsupportedSchema(99))
    ));
}

#[test]
fn late_migration_failure_rolls_back_all_slice4_work() {
    let path = v2_fixture();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE compatibility_decisions SET result=x'01' WHERE id=1",
        [],
    )
    .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT typeof(result) FROM compatibility_decisions WHERE id=1",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "blob"
    );
    for object in SLICE4 {
        assert!(!object_exists(&db, "table", object));
    }
    assert!(!object_exists(&db, "table", "compatibility_decisions_v2"));
    assert!(!object_exists(
        &db,
        "index",
        "reconciliation_runs_scope_sequence"
    ));
    assert!(!object_exists(
        &db,
        "trigger",
        "immutable_reconciliation_runs_update"
    ));
    fs::remove_file(path).unwrap();
}

#[test]
fn slice4_action_and_outcome_rows_are_immutable_and_schema_versioned() {
    let store = Store::open_in_memory().unwrap();
    let db = store.connection();
    db.execute("INSERT INTO reconciliation_runs(scope_id,run_sequence,status,source_lineage) VALUES('s',1,'clean','native')", []).unwrap();
    db.execute("INSERT INTO reconciliation_discrepancies(run_id,scope_id,source_lineage,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(1,'s','native','filesystem','p',0,'now')", []).unwrap();
    db.execute("INSERT INTO recovery_actions(operation_id,run_id,discrepancy_id,scope_id,fingerprint_version,fingerprint,target_identity,authorization_identity,authorization_scope,stage,sequence,source_lineage) VALUES('op',1,1,'s',1,'fp','target','auth','scope','prepared',0,'native')", []).unwrap();
    db.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,detail,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(1,1,'s',1,'effect_not_started',NULL,'native','effect_observed','effect_not_started','pre',NULL,'action','op',0,'now')", []).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM recovery_actions WHERE fingerprint_version=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let update = db
        .execute(
            "UPDATE recovery_actions SET authorization_scope='other' WHERE action_id=1",
            [],
        )
        .unwrap_err();
    assert!(update.to_string().contains("immutable"));
    let delete = db
        .execute("DELETE FROM recovery_outcomes WHERE outcome_id=1", [])
        .unwrap_err();
    assert!(delete.to_string().contains("immutable"));
}

fn canonical_chain_db() -> Store {
    let store = Store::open_in_memory().unwrap();
    let db = store.connection();
    db.execute("INSERT INTO reconciliation_runs(scope_id,run_sequence,status,source_lineage) VALUES('scope',1,'blocked','lineage')", []).unwrap();
    db.execute("INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(10,1,'scope','lineage','filesystem','original',0,'t0')", []).unwrap();
    db.execute("INSERT INTO recovery_actions(action_id,operation_id,run_id,discrepancy_id,scope_id,fingerprint_version,fingerprint,target_identity,authorization_identity,authorization_scope,stage,sequence,source_lineage) VALUES(20,'op',1,10,'scope',1,'fp','target','auth','scope','prepared',0,'lineage')", []).unwrap();
    store
}

#[test]
fn legal_effect_terminal_correction_chain_uses_exact_immediate_predecessors() {
    let store = canonical_chain_db();
    let db = store.connection();
    db.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',1,'effect_applied','lineage','effect_observed','effect_applied','pre','post','action','op',1,'t1')", []).unwrap();
    db.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at,supersedes_id,supersedes_action_id) VALUES(20,1,'scope',2,'applied','lineage','terminal','applied','pre','post','action','op',1,'t2',1,20)", []).unwrap();
    db.execute("INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',10,1,'action','op',2,'t3')", []).unwrap();
    db.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at,supersedes_id,supersedes_action_id) VALUES(20,1,'scope',3,'same','lineage','correction','same','pre','post','reconciliation_discrepancy','11',2,'t3',2,20)", []).unwrap();
    assert_eq!(db.query_row("SELECT group_concat(sequence, ',') FROM recovery_outcomes WHERE action_id=20 ORDER BY sequence", [], |r| r.get::<_, String>(0)).unwrap(), "1,2,3");
    // An older-head attempt is rejected by exact immediate-predecessor lineage;
    // UNIQUE(action_id,supersedes_id) remains the linear-head constraint.
    assert_eq!(db.query_row("SELECT count(*) FROM recovery_outcomes WHERE action_id=20 AND outcome_id NOT IN (SELECT supersedes_id FROM recovery_outcomes WHERE supersedes_id IS NOT NULL)", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT run_id||':'||scope_id||':'||source_lineage FROM recovery_outcomes WHERE sequence=3", [], |r| r.get::<_, String>(0)).unwrap(), "1:scope:lineage");
}

#[test]
fn untested_correction_guards_reject_only_the_named_invalid_chain() {
    invalid_case(
        "INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',10,1,'action','op',1,'now'); INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(12,1,'scope','lineage',10,1,'action','op',2,'now')",
        "invalid correction",
    );
    invalid_case(
        "INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',99,1,'action','op',1,'now')",
        "invalid correction",
    );
    invalid_case(
        "INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','other',10,1,'action','op',1,'now')",
        "invalid correction",
    );

    let store = canonical_chain_db();
    let db = store.connection();
    db.execute("INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',10,1,'action','op',1,'now')", []).unwrap();
    outcome(
        db,
        20,
        1,
        "effect_applied",
        "effect_observed",
        Some("pre"),
        Some("post"),
        None,
        None,
        "op",
    )
    .unwrap();
    outcome(
        db,
        20,
        2,
        "applied",
        "terminal",
        Some("pre"),
        Some("post"),
        Some(1),
        Some(20),
        "op",
    )
    .unwrap();
    let error = outcome(
        db,
        20,
        4,
        "same",
        "correction",
        Some("pre"),
        Some("post"),
        Some(2),
        Some(20),
        "11",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("invalid correction outcome"),
        "invalid correction outcome: {error}"
    );

    let store = canonical_chain_db();
    let db = store.connection();
    db.execute("INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',10,1,'action','op',1,'now')", []).unwrap();
    outcome(
        db,
        20,
        1,
        "effect_applied",
        "effect_observed",
        Some("pre"),
        Some("post"),
        None,
        None,
        "op",
    )
    .unwrap();
    outcome(
        db,
        20,
        2,
        "applied",
        "terminal",
        Some("pre"),
        Some("post"),
        Some(1),
        Some(20),
        "op",
    )
    .unwrap();
    outcome(
        db,
        20,
        3,
        "same",
        "correction",
        Some("pre"),
        Some("post"),
        Some(2),
        Some(20),
        "11",
    )
    .unwrap();
    let error = outcome(
        db,
        20,
        4,
        "same",
        "correction",
        Some("pre"),
        Some("post"),
        Some(2),
        Some(20),
        "11",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("invalid correction outcome"),
        "invalid correction outcome: {error}"
    );

    let store = canonical_chain_db();
    let db = store.connection();
    db.execute("INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',10,1,'action','op',1,'now')", []).unwrap();
    db.execute("INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(12,1,'scope','lineage','filesystem','separate',1,'now')", []).unwrap();
    outcome(
        db,
        20,
        1,
        "effect_applied",
        "effect_observed",
        Some("pre"),
        Some("post"),
        None,
        None,
        "op",
    )
    .unwrap();
    outcome(
        db,
        20,
        2,
        "applied",
        "terminal",
        Some("pre"),
        Some("post"),
        Some(1),
        Some(20),
        "op",
    )
    .unwrap();
    let error = outcome(
        db,
        20,
        3,
        "same",
        "correction",
        Some("pre"),
        Some("post"),
        Some(2),
        Some(20),
        "12",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("invalid correction outcome"),
        "invalid correction outcome: {error}"
    );
}

#[test]
fn all_slice4_immutable_triggers_report_exact_table_message() {
    let store = canonical_chain_db();
    let db = store.connection();
    db.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',1,'effect_not_started','lineage','effect_observed','effect_not_started','pre',NULL,'action','op',0,'now')", []).unwrap();
    let cases = [
        (
            "reconciliation_runs",
            "UPDATE reconciliation_runs SET status='x' WHERE id=1",
            "UPDATE",
        ),
        (
            "reconciliation_discrepancies",
            "UPDATE reconciliation_discrepancies SET reason='x' WHERE id=10",
            "UPDATE",
        ),
        (
            "recovery_actions",
            "UPDATE recovery_actions SET authorization_scope='x' WHERE action_id=20",
            "UPDATE",
        ),
        (
            "recovery_outcomes",
            "UPDATE recovery_outcomes SET detail='x' WHERE outcome_id=1",
            "UPDATE",
        ),
    ];
    for (table, sql, _) in cases {
        let error = db.execute(sql, []).unwrap_err();
        assert_eq!(error.to_string(), format!("immutable {table}"));
    }
    for (table, sql) in [
        (
            "reconciliation_runs",
            "DELETE FROM reconciliation_runs WHERE id=1",
        ),
        (
            "reconciliation_discrepancies",
            "DELETE FROM reconciliation_discrepancies WHERE id=10",
        ),
        (
            "recovery_actions",
            "DELETE FROM recovery_actions WHERE action_id=20",
        ),
        (
            "recovery_outcomes",
            "DELETE FROM recovery_outcomes WHERE outcome_id=1",
        ),
    ] {
        let store = canonical_chain_db();
        let db = store.connection();
        if table == "recovery_outcomes" {
            db.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',1,'effect_not_started','lineage','effect_observed','effect_not_started','pre',NULL,'action','op',0,'now')", []).unwrap();
        }
        let error = db.execute(sql, []).unwrap_err();
        assert_eq!(error.to_string(), format!("immutable {table}"));
    }
}

#[allow(clippy::too_many_arguments)]
fn outcome(
    db: &Connection,
    action: i64,
    sequence: i64,
    result: &str,
    stage: &str,
    pre: Option<&str>,
    post: Option<&str>,
    supersedes: Option<i64>,
    supersedes_action: Option<i64>,
    provenance: &str,
) -> rusqlite::Result<usize> {
    db.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at,supersedes_id,supersedes_action_id) VALUES(?1,1,'scope',?2,?3,'lineage',?4,?3,?5,?6,'reconciliation_discrepancy',?7,0,'now',?8,?9)", rusqlite::params![action, sequence, result, stage, pre, post, provenance, supersedes, supersedes_action])
}

fn invalid_case(sql: &str, reason: &str) {
    let store = canonical_chain_db();
    let error = store.connection().execute_batch(sql).unwrap_err();
    assert!(error.to_string().contains(reason), "{reason}: {error}");
}

fn effect_case(result: &str, pre: Option<&str>, post: Option<&str>) -> String {
    format!("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',1,'{result}','lineage','effect_observed','{result}',{},{},'action','op',0,'now')", pre.map_or("NULL".into(), |v| format!("'{v}'")), post.map_or("NULL".into(), |v| format!("'{v}'")))
}

fn terminal_case(effect: &str, terminal: &str, reason: &str) {
    let store = canonical_chain_db();
    let db = store.connection();
    db.execute_batch(&effect_case(
        effect,
        Some("pre"),
        if effect == "effect_not_started" {
            None
        } else {
            Some("post")
        },
    ))
    .unwrap();
    let sql = format!("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at,supersedes_id,supersedes_action_id) VALUES(20,1,'scope',2,'{terminal}','lineage','terminal','{terminal}','pre','post','action','op',0,'now',1,20)");
    let error = db.execute_batch(&sql).unwrap_err();
    assert!(error.to_string().contains(reason), "{reason}: {error}");
}

#[test]
fn illegal_slice4_matrix_is_rejected_by_the_named_guard() {
    for (sql, reason) in [
        ("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',2,'applied','lineage','terminal','applied','pre','post','action','op',0,'now')", "invalid terminal"),
        ("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',1,'effect_applied','lineage','effect_observed','effect_applied',NULL,'post','action','op',0,'now')", "invalid observations"),
        ("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',1,'effect_not_started','lineage','effect_observed','effect_not_started','pre','post','action','op',0,'now')", "invalid observations"),
        ("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,1,'scope',1,'effect_unknown','lineage','effect_observed','effect_unknown','pre',NULL,'action','op',0,'now')", "invalid observations"),
        ("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at,supersedes_id,supersedes_action_id) VALUES(20,1,'scope',3,'same','lineage','correction','same','pre','post','reconciliation_discrepancy','11',0,'now',NULL,NULL)", "invalid correction outcome"),
    ] { invalid_case(sql, reason); }

    let store = canonical_chain_db();
    let db = store.connection();
    outcome(
        db,
        20,
        1,
        "effect_not_started",
        "effect_observed",
        Some("pre"),
        None,
        None,
        None,
        "op",
    )
    .unwrap();
    let err = outcome(
        db,
        20,
        2,
        "applied",
        "terminal",
        Some("pre"),
        Some("post"),
        Some(1),
        Some(20),
        "op",
    )
    .unwrap_err();
    assert!(err.to_string().contains("wrong terminal mapping"));
}

#[test]
fn complete_slice4_illegal_matrix_is_isolated_and_named() {
    for (effect, terminals) in [
        ("effect_applied", &["blocked", "unknown", "same"] as &[&str]),
        ("effect_not_started", &["applied", "unknown"]),
        ("effect_unknown", &["applied", "blocked", "same"]),
    ] {
        for terminal in terminals {
            terminal_case(effect, terminal, "wrong terminal mapping");
        }
    }
    for (result, pre, post) in [
        ("effect_applied", Some("pre"), None),
        ("effect_not_started", None, None),
        ("effect_not_started", Some("pre"), Some("post")),
        ("effect_unknown", Some("pre"), None),
    ] {
        invalid_case(&effect_case(result, pre, post), "invalid observations");
    }

    for (run, scope, lineage, reason) in [
        (2, "scope", "lineage", "action discrepancy mismatch"),
        (1, "other", "lineage", "action discrepancy mismatch"),
        (1, "scope", "other", "action discrepancy mismatch"),
    ] {
        invalid_case(&format!("INSERT INTO recovery_actions(operation_id,run_id,discrepancy_id,scope_id,fingerprint_version,fingerprint,target_identity,authorization_identity,authorization_scope,stage,sequence,source_lineage) VALUES('op',{run},10,'{scope}',1,'bad','target','auth','scope','prepared',0,'{lineage}')"), reason);
    }
    for (run, scope, lineage) in [
        (2, "scope", "lineage"),
        (1, "other", "lineage"),
        (1, "scope", "other"),
    ] {
        invalid_case(&format!("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(20,{run},'{scope}',1,'effect_not_started','{lineage}','effect_observed','effect_not_started','pre',NULL,'action','op',0,'now')"), "outcome action mismatch");
    }
    invalid_case("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at,supersedes_id,supersedes_action_id) VALUES(20,1,'scope',2,'blocked','lineage','terminal','blocked','pre',NULL,'action','op',0,'now',99,20)", "invalid terminal");

    for sql in [
        "INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',10,'action','op',1,'now')",
        "INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'scope','lineage',10,2,'action','op',1,'now')",
        "INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(11,1,'other','lineage',10,1,'action','op',1,'now')",
        "INSERT INTO reconciliation_discrepancies(id,run_id,scope_id,source_lineage,correction_of_id,correction_of_run_id,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(10,1,'scope','lineage',10,1,'action','op',1,'now')",
    ] { invalid_case(sql, if sql.contains("correction_of_run_id") || sql.contains("VALUES(10") { "invalid correction" } else { "correction parent pair" }); }
}

#[test]
fn old_development_v3_layout_is_rejected_without_mutation() {
    let path = fresh_path("old-development-v3");
    let store = Store::open(&path).unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE recovery_outcomes; CREATE TABLE recovery_outcomes(outcome_id INTEGER PRIMARY KEY,schema_version INTEGER NOT NULL DEFAULT 1 CHECK(schema_version=1),action_id INTEGER NOT NULL,run_id INTEGER NOT NULL,scope_id TEXT NOT NULL,sequence INTEGER NOT NULL CHECK(sequence>=0),status TEXT NOT NULL,detail TEXT,source_lineage TEXT NOT NULL, UNIQUE(action_id,sequence), FOREIGN KEY(action_id,run_id) REFERENCES recovery_actions(action_id,run_id)); CREATE INDEX recovery_outcomes_action_sequence ON recovery_outcomes(action_id,sequence); CREATE TRIGGER recovery_outcomes_immutable_update BEFORE UPDATE ON recovery_outcomes BEGIN SELECT RAISE(ABORT,'immutable recovery_outcomes'); END; CREATE TRIGGER recovery_outcomes_immutable_delete BEFORE DELETE ON recovery_outcomes BEGIN SELECT RAISE(ABORT,'immutable recovery_outcomes'); END;").unwrap();
    drop(db);
    let bytes = fs::read(&path).unwrap();
    let inventory = physical_snapshot(&Connection::open(&path).unwrap());
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        physical_snapshot(&Connection::open(&path).unwrap()),
        inventory
    );
    fs::remove_file(path).unwrap();
}
