//! RED contract tests for build-artifact-runtime Slice 2 (tasks 3.1–3.4).
//! These intentionally target the durable-record API that is not implemented yet.
use crate::artifacts;

use artifacts::Store;
use rusqlite::{Connection, OptionalExtension};

fn database() -> Connection {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice2-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = Store::open(&path).expect("artifact store");
    drop(store);
    Connection::open(path).expect("sqlite connection")
}

fn has_table(db: &Connection, name: &str) -> bool {
    db.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .expect("schema query")
    .is_some()
}

#[test]
fn every_runtime_record_is_schema_versioned_and_append_only() {
    let db = database();
    for table in [
        "events",
        "findings",
        "evidence",
        "history",
        "replay_metadata",
    ] {
        assert!(has_table(&db, table), "missing durable table: {table}");
        let column: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name='schema_version'",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(column, 1, "{table} lacks explicit schema_version");
    }
}

#[test]
fn durable_history_rejects_update_and_delete_but_allows_corrections() {
    let db = database();
    assert!(has_table(&db, "events"));
    db.execute(
        "INSERT INTO events(id,schema_version,source_lineage) VALUES('e1',1,'source')",
        [],
    )
    .expect("new event");
    assert!(db
        .execute(
            "UPDATE events SET source_lineage='repair' WHERE id='e1'",
            []
        )
        .is_err());
    assert!(db.execute("DELETE FROM events WHERE id='e1'", []).is_err());
    db.execute("INSERT INTO events(id,schema_version,source_lineage,correction_of) VALUES('e2',1,'source','e1')", [])
        .expect("correction is a new record");
}

#[test]
fn unsupported_or_ambiguous_record_schema_fails_closed_and_keeps_lineage() {
    let db = database();
    assert!(has_table(&db, "events"));
    assert!(db
        .execute(
            "INSERT INTO events(id,schema_version,source_lineage) VALUES('bad',999,'origin')",
            []
        )
        .is_err());
    assert!(db.execute("INSERT INTO events(id,schema_version,source_lineage) VALUES('ambiguous',NULL,'origin')", []).is_err());
}

#[test]
fn lifecycle_evidence_records_status_actor_reason_and_operation_lineage() {
    let db = database();
    assert!(has_table(&db, "lifecycle_evidence"));
    for status in ["created", "invalidated"] {
        db.execute("INSERT INTO lifecycle_evidence(id,schema_version,status,actor,reason,content_version,operation_lineage) VALUES(?1,1,?1,'reviewer','reason','v1','op1')", [&status])
            .expect("lifecycle evidence");
    }
}

#[test]
fn reviewer_findings_use_bounded_append_only_state_transitions() {
    let db = database();
    assert!(has_table(&db, "reviewer_findings"));
    for state in [
        "open",
        "acknowledged",
        "fixed",
        "waived",
        "rejected",
        "stale",
    ] {
        db.execute(
            "INSERT INTO reviewer_findings(id,schema_version,state) VALUES(?1,1,?1)",
            [&state],
        )
        .expect("bounded finding state");
    }
    assert!(db
        .execute(
            "INSERT INTO reviewer_findings(id,schema_version,state) VALUES('invalid',1,'unknown')",
            []
        )
        .is_err());
}

#[test]
fn terminal_outcomes_are_bounded_and_retrospective_acceptance_is_ordered() {
    let db = database();
    assert!(has_table(&db, "outcomes"));
    for outcome in [
        "verified",
        "accepted_with_waivers",
        "accepted_partial",
        "blocked",
        "aborted",
        "failed",
    ] {
        db.execute(
            "INSERT INTO outcomes(id,schema_version,outcome) VALUES(?1,1,?1)",
            [&outcome],
        )
        .expect("bounded outcome");
    }
    assert!(db
        .execute(
            "INSERT INTO outcomes(id,schema_version,outcome) VALUES('invalid',1,'succeeded')",
            []
        )
        .is_err());
    assert!(has_table(&db, "retrospectives"));
    assert!(has_table(&db, "acceptance_requirements"));
}
