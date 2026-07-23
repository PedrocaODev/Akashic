//! Contract tests for the completed build-artifact-runtime Slice 3 (tasks 4.1–5.4).
//! These verify the public persistence boundary.
use crate::artifacts;

use artifacts::{Event, Store};
use rusqlite::{Connection, OptionalExtension};

fn database() -> Connection {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-{}-{}.sqlite",
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

fn has_columns(db: &Connection, table: &str, columns: &[&str]) -> bool {
    columns.iter().all(|column| {
        db.query_row(
            "SELECT 1 FROM pragma_table_info(?1) WHERE name=?2",
            [table, column],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .expect("column query")
        .is_some()
    })
}

#[test]
fn projections_identify_schema_generations_and_visible_drift() {
    let db = database();
    assert!(has_table(&db, "projections"));
    assert!(has_columns(
        &db,
        "projections",
        &[
            "id",
            "projection_schema_version",
            "event_history_generation",
            "source_generation",
            "authoritative",
            "status",
        ],
    ));
    for status in ["missing", "stale", "drifted", "complete"] {
        db.execute(
            "INSERT INTO projections(id,projection_schema_version,event_history_generation,source_generation,authoritative,status) VALUES(?1,1,1,1,0,?1)",
            [status],
        )
        .expect("projection status");
    }
}

#[test]
fn rebuild_orders_compatible_events_and_replaces_only_after_success() {
    let db = database();
    assert!(has_table(&db, "projection_rebuilds"));
    assert!(has_columns(
        &db,
        "projection_rebuilds",
        &[
            "projection_id",
            "event_id",
            "event_sequence",
            "status",
            "failure",
            "drift"
        ],
    ));
    assert!(has_table(&db, "projection_rebuild_status"));
    db.execute(
        "INSERT INTO projection_rebuild_status(projection_id,status,authoritative,failure,drift) VALUES('p1','failed',0,'bad event','generation drift')",
        [],
    )
    .expect("visible failed rebuild");
}

#[test]
fn exact_playback_reapplies_deterministic_events_and_records_divergence() {
    let db = database();
    assert!(has_table(&db, "playback_results"));
    assert!(has_columns(
        &db,
        "playback_results",
        &[
            "id",
            "event_history_generation",
            "result",
            "divergence",
            "exact"
        ],
    ));
    for result in ["exact_playback_succeeded", "diverged"] {
        db.execute(
            "INSERT INTO playback_results(id,event_history_generation,result,divergence,exact) VALUES(?1,1,?1,?2,?3)",
            rusqlite::params![result, (result == "diverged").then_some("diverged"), result == "exact_playback_succeeded"],
        )
        .expect("playback result");
    }
}

#[test]
fn captured_outcome_is_separate_and_cannot_claim_exact_playback() {
    let db = database();
    assert!(has_table(&db, "captured_outcome_simulations"));
    assert!(has_columns(
        &db,
        "captured_outcome_simulations",
        &["id", "label", "outcome", "exact_playback_evidence"],
    ));
    db.execute(
        "INSERT INTO captured_outcome_simulations(id,label,outcome,exact_playback_evidence) VALUES('sim-1','captured_outcome_simulation','accepted',0)",
        [],
    )
    .expect("simulation is separately stored");
}

#[test]
fn behavior_rebuilds_deterministically_and_replaces_only_on_success() {
    let store = Store::open_in_memory().expect("store");
    store
        .append_event(Event::new("e1", 1, "t1", "a", "A"))
        .unwrap();
    store
        .append_event(Event::new("e2", 2, "t2", "b", "B"))
        .unwrap();
    let rebuilt = store.rebuild_projection("p", 1).expect("rebuild");
    assert_eq!(rebuilt.status, "complete");
    assert!(rebuilt.authoritative);
    assert_eq!(rebuilt.payload, "A\nB");
    assert_ne!(rebuilt.event_history_generation, 2);
    let generation = rebuilt.event_history_generation;
    assert_eq!(rebuilt.projection_schema_version, 1);
    assert!(matches!(
        store.append_event(Event::new("e0", 0, "t1", "zero", "ZERO")),
        Err(artifacts::Error::OutOfOrder)
    ));
    let stale = store.projection("p").unwrap().unwrap();
    assert_eq!(stale.event_history_generation, generation);
    assert_eq!(store.projection_status("p").unwrap(), "complete");
    let failed = store.rebuild_projection("p", 99).expect_err("incompatible");
    assert!(matches!(failed, artifacts::Error::RebuildFailure));
    let current = store.projection("p").unwrap().unwrap();
    assert_eq!(current.payload, "A\nB");
    assert!(!current.authoritative);
}

#[test]
fn event_history_generation_is_stable_across_reopen_and_changes_with_history() {
    let path = std::env::temp_dir().join(format!(
        "akashic-slice3-generation-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let generation = {
        let store = Store::open(&path).unwrap();
        store
            .append_event(Event::new("e1", 1, "t1", "a", "A"))
            .unwrap();
        store
            .rebuild_projection("p", 1)
            .unwrap()
            .event_history_generation
    };
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened
            .projection("p")
            .unwrap()
            .unwrap()
            .event_history_generation,
        generation
    );
    assert_eq!(
        reopened
            .rebuild_projection("p", 1)
            .unwrap()
            .event_history_generation,
        generation
    );
    reopened
        .append_event(Event::new("e2", 2, "t2", "b", "B"))
        .unwrap();
    let changed = reopened
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    assert_ne!(changed, generation);
}

#[test]
fn playback_distinguishes_exact_divergence_and_captured_outcomes() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e1", 1, "t1", "a", "A"))
        .unwrap();
    let generation = store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    assert!(
        store
            .playback("run1", generation, &["t1"], &["A"])
            .unwrap()
            .exact
    );
    let divergent = store
        .playback("run2", generation, &["wrong"], &["A"])
        .unwrap();
    assert_eq!(divergent.result, "diverged");
    assert!(!divergent.exact);
    let simulation = store.simulate_captured_outcome("sim", "accepted").unwrap();
    assert_eq!(simulation.label, "captured_outcome_simulation");
    assert!(!simulation.exact_playback_evidence);
    assert!(store.exact_playback_evidence("sim").is_err());
}

#[test]
fn exact_playback_evidence_requires_persisted_coherent_exact_result() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e1", 1, "identity", "a", "a"))
        .unwrap();
    let generation = store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    store
        .playback("exact", generation, &["identity"], &["a"])
        .unwrap();
    assert!(store.exact_playback_evidence("exact").is_ok());
    assert!(store.exact_playback_evidence("missing").is_err());
    store
        .playback("diverged", generation, &["identity"], &["wrong"])
        .unwrap();
    assert!(store.exact_playback_evidence("diverged").is_err());
    store.simulate_captured_outcome("sim", "a").unwrap();
    assert!(store.exact_playback_evidence("sim").is_err());
}

#[test]
fn playback_uses_selected_generation_and_derived_results_not_echoed_outcomes() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e1", 1, "uppercase", "a", "tampered"))
        .unwrap();
    let tampered = store
        .playback_with_inputs(
            "run",
            store
                .rebuild_projection("p", 1)
                .unwrap()
                .event_history_generation,
            &["uppercase"],
            &["a"],
            &["A"],
        )
        .unwrap();
    assert!(!tampered.exact);
    assert_eq!(tampered.result, "diverged");
    assert_eq!(
        tampered.divergence.as_deref(),
        Some("persisted outcome mismatch")
    );
    let short = store
        .playback_with_inputs("short", 1, &[], &[], &[])
        .unwrap();
    assert!(!short.exact);
    assert_eq!(short.result, "diverged");
    let wrong_generation = store
        .playback_with_inputs("generation", 99, &["uppercase"], &["a"], &["A"])
        .unwrap();
    assert!(!wrong_generation.exact);
}

#[test]
fn playback_accepts_a_persisted_historical_prefix_generation() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e1", 1, "uppercase", "a", "A"))
        .unwrap();
    let historical = store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    store
        .append_event(Event::new("e2", 2, "uppercase", "b", "B"))
        .unwrap();
    let replay = store
        .playback_with_inputs("historical", historical, &["uppercase"], &["a"], &["A"])
        .unwrap();
    assert!(replay.exact);
    assert_eq!(replay.result, "exact_playback_succeeded");
}

#[test]
fn playback_accepts_only_returned_hash_generation_not_event_count() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e1", 1, "uppercase", "a", "A"))
        .unwrap();
    let generation = store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;

    assert!(
        store
            .playback("exact", generation, &["uppercase"], &["A"])
            .unwrap()
            .exact
    );
    assert!(
        !store
            .playback("count", 1, &["uppercase"], &["A"])
            .unwrap()
            .exact
    );
    assert!(
        !store
            .playback("other", generation + 1, &["uppercase"], &["A"])
            .unwrap()
            .exact
    );
}

#[test]
fn projection_status_reports_missing_stale_and_drifted_without_authority() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.projection_status("missing").unwrap(), "missing");
    store
        .append_event(Event::new("e1", 1, "uppercase", "a", "A"))
        .unwrap();
    assert_eq!(store.projection_status("missing").unwrap(), "missing");
    store.rebuild_projection("p", 1).unwrap();
    assert_eq!(store.projection_status("p").unwrap(), "complete");
    store
        .append_event(Event::new("e2", 2, "uppercase", "b", "B"))
        .unwrap();
    assert_eq!(store.projection_status("p").unwrap(), "stale");
}

#[test]
fn direct_projection_read_reconciles_stale_schema_and_payload() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e1", 1, "uppercase", "a", "A"))
        .unwrap();
    store.rebuild_projection("p", 1).unwrap();
    store
        .connection()
        .execute(
            "UPDATE projections SET projection_schema_version=99 WHERE id='p'",
            [],
        )
        .unwrap();
    let schema = store.projection("p").unwrap().unwrap();
    assert_eq!(schema.status, "stale");
    assert!(!schema.authoritative);

    store
        .append_event(Event::new("e2", 2, "uppercase", "b", "B"))
        .unwrap();
    let stale = store.projection("p").unwrap().unwrap();
    assert_eq!(stale.status, "stale");
    assert!(!stale.authoritative);

    store.rebuild_projection("p", 1).unwrap();
    store.connection().execute("UPDATE projections SET projection_schema_version=1, payload='wrong', status='complete', authoritative=1 WHERE id='p'", []).unwrap();
    let drifted = store.projection("p").unwrap().unwrap();
    assert_eq!(drifted.status, "drifted");
    assert!(!drifted.authoritative);
}

#[test]
fn malformed_slice3_schema_is_rejected_at_startup() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE schema_metadata(key TEXT PRIMARY KEY,value INTEGER NOT NULL); INSERT INTO schema_metadata VALUES('schema_version',1),('migration_lineage',1);").unwrap();
    assert!(Store::from_connection(db).is_err());
}

#[test]
fn playback_rejects_persisted_event_and_unknown_transition_mismatches() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("unrelated", 9, "uppercase", "a", "A"))
        .unwrap();
    assert!(
        !store
            .playback_with_inputs("mismatch", 1, &["identity"], &["a"], &["a"])
            .unwrap()
            .exact
    );
    assert!(
        !store
            .playback_with_inputs("unknown", 1, &["no-such-transition"], &["a"], &["a"])
            .unwrap()
            .exact
    );
}

#[test]
fn playback_rejects_every_length_mismatch_without_panicking() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "uppercase", "a", "A"))
        .unwrap();
    for (n, (t, i, o)) in [
        (&["uppercase"][..], &[][..], &["A"][..]),
        (&[][..], &["a"][..], &["A"][..]),
        (&["uppercase"][..], &["a"][..], &[][..]),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            !store
                .playback_with_inputs(&format!("length-{n}"), 1, t, i, o)
                .unwrap()
                .exact
        );
    }
}

#[test]
fn playback_requires_all_caller_sequences_to_match_selected_history_length() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "uppercase", "a", "A"))
        .unwrap();
    store
        .append_event(Event::new("e2", 2, "uppercase", "b", "B"))
        .unwrap();
    let generation = store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    let longer = store
        .playback_with_inputs(
            "longer",
            generation,
            &["uppercase", "uppercase", "uppercase"],
            &["a", "b", "c"],
            &["A", "B", "C"],
        )
        .unwrap();
    assert!(!longer.exact);
    let recorded: i64 = store
        .connection()
        .query_row(
            "SELECT exact FROM playback_results WHERE id='longer'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recorded, 0);
}

#[test]
fn null_and_empty_outcomes_have_distinct_generations() {
    let null_store = Store::open_in_memory().unwrap();
    null_store.connection().execute(
        "INSERT INTO events(id,schema_version,source_lineage,event_sequence,transition,deterministic_input,outcome) VALUES('e',1,'native',1,'identity','a',NULL)", [],
    ).unwrap();
    let null_generation = null_store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    let empty_store = Store::open_in_memory().unwrap();
    empty_store.connection().execute(
        "INSERT INTO events(id,schema_version,source_lineage,event_sequence,transition,deterministic_input,outcome) VALUES('e',1,'native',1,'identity','a','')", [],
    ).unwrap();
    let empty_generation = empty_store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    assert_ne!(null_generation, empty_generation);
}

#[test]
fn partial_slice3_schema_fails_closed_instead_of_being_repaired() {
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE schema_metadata(key TEXT PRIMARY KEY,value INTEGER NOT NULL); INSERT INTO schema_metadata VALUES('schema_version',1),('migration_lineage',1); CREATE TABLE events(id TEXT PRIMARY KEY);").unwrap();
    assert!(matches!(
        Store::from_connection(db),
        Err(artifacts::Error::AmbiguousSchema)
    ));
}

#[test]
fn drift_and_stale_status_are_persisted_and_remove_authority() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "uppercase", "a", "A"))
        .unwrap();
    store.rebuild_projection("p", 1).unwrap();
    store
        .append_event(Event::new("e2", 2, "uppercase", "b", "B"))
        .unwrap();
    assert_eq!(store.projection_status("p").unwrap(), "stale");
    assert!(!store.projection("p").unwrap().unwrap().authoritative);
}

#[test]
fn startup_rejects_missing_result_guard_and_mutable_result_constraints() {
    let db = database();
    db.execute("DROP TRIGGER immutable_playback_results_update", [])
        .unwrap();
    assert!(Store::from_connection(db).is_err());
}

#[test]
fn playback_result_constraints_reject_incoherent_rows() {
    let db = database();
    assert!(db.execute("INSERT INTO playback_results(id,event_history_generation,result,divergence,exact) VALUES('bad-exact',1,'exact_playback_succeeded','why',1)", []).is_err());
    assert!(db.execute("INSERT INTO playback_results(id,event_history_generation,result,divergence,exact) VALUES('bad-diverged',1,'diverged',NULL,0)", []).is_err());
}

#[test]
fn startup_rejects_preexisting_event_history_without_immutability_guards() {
    let db = database();
    db.execute("DROP TRIGGER immutable_events_update", [])
        .unwrap();
    assert!(Store::from_connection(db).is_err());
}

#[test]
fn failed_rebuild_keeps_payload_but_publishes_no_authoritative_result() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "uppercase", "a", "A"))
        .unwrap();
    store.rebuild_projection("p", 1).unwrap();
    assert!(store.rebuild_projection("p", 99).is_err());
    let current = store.projection("p").unwrap().unwrap();
    assert_eq!(current.payload, "A");
    assert!(!current.authoritative);
}

#[test]
fn null_persisted_outcome_is_absent_and_never_exact() {
    let store = Store::open_in_memory().unwrap();
    store.connection().execute("INSERT INTO events(id,schema_version,source_lineage,event_sequence,transition,deterministic_input,outcome) VALUES('e',1,'native',1,'uppercase','a',NULL)", []).unwrap();
    let generation = store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    assert!(
        !store
            .playback("null", generation, &["uppercase"], &["A"])
            .unwrap()
            .exact
    );
}

#[test]
fn event_generation_distinguishes_null_sentinel_and_nul_boundaries() {
    fn generation(outcome: Option<&str>, input: &str) -> i64 {
        let store = Store::open_in_memory().unwrap();
        store.connection().execute("INSERT INTO events(id,schema_version,source_lineage,event_sequence,transition,deterministic_input,outcome) VALUES('e',1,'native',1,'identity',?1,?2)", rusqlite::params![input, outcome]).unwrap();
        store
            .rebuild_projection("p", 1)
            .unwrap()
            .event_history_generation
    }
    assert_ne!(generation(None, "x"), generation(Some("<NULL>"), "x"));
    assert_ne!(generation(Some("b"), "a\0b"), generation(Some("a\0b"), ""));
}

#[test]
fn successful_rebuild_clears_prior_drift_metadata() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "uppercase", "a", "A"))
        .unwrap();
    store.rebuild_projection("p", 1).unwrap();
    store
        .connection()
        .execute(
            "UPDATE projection_rebuild_status SET status='drifted', authoritative=0, drift='payload drift' WHERE projection_id='p'",
            [],
        )
        .unwrap();

    let rebuilt = store.rebuild_projection("p", 1).unwrap();

    assert_eq!(rebuilt.status, "complete");
    assert!(rebuilt.authoritative);
    let metadata: (String, i64, Option<String>) = store
        .connection()
        .query_row(
            "SELECT status, authoritative, drift FROM projection_rebuild_status WHERE projection_id='p'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(metadata, ("complete".into(), 1, None));
}

#[test]
fn projection_status_repairs_persisted_failed_authority() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "uppercase", "a", "A"))
        .unwrap();
    store.rebuild_projection("p", 1).unwrap();
    store
        .connection()
        .execute(
            "UPDATE projections SET status='failed', authoritative=0 WHERE id='p'",
            [],
        )
        .unwrap();

    assert_eq!(store.projection_status("p").unwrap(), "failed");
    let projection = store.projection("p").unwrap().unwrap();
    assert!(!projection.authoritative);
}

#[test]
fn persisted_payload_drift_updates_status_and_authority() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "uppercase", "a", "A"))
        .unwrap();
    store.rebuild_projection("p", 1).unwrap();
    store
        .connection()
        .execute("UPDATE projections SET payload='wrong' WHERE id='p'", [])
        .unwrap();

    assert_eq!(store.projection_status("p").unwrap(), "drifted");
    let projection = store.projection("p").unwrap().unwrap();
    assert_eq!(projection.status, "drifted");
    assert!(!projection.authoritative);
}

#[test]
fn unknown_transition_rebuild_publishes_failed_status_before_returning() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e", 1, "unknown", "a", "A"))
        .unwrap();
    assert!(matches!(
        store.rebuild_projection("p", 1),
        Err(artifacts::Error::RebuildFailure)
    ));
    let status = store.projection_status("p").unwrap();
    assert_eq!(status, "failed");
    let projection = store.projection("p");
    assert!(projection.is_ok());
}

#[test]
fn append_rejects_historical_sequences_but_preserves_replayable_generation() {
    let store = Store::open_in_memory().unwrap();
    store
        .append_event(Event::new("e1", 1, "uppercase", "a", "A"))
        .unwrap();
    let generation = store
        .rebuild_projection("p", 1)
        .unwrap()
        .event_history_generation;
    store
        .append_event(Event::new("e2", 2, "uppercase", "b", "B"))
        .unwrap();
    assert!(matches!(
        store.append_event(Event::new("old", 1, "uppercase", "x", "X")),
        Err(artifacts::Error::OutOfOrder)
    ));
    assert_eq!(
        store
            .connection()
            .query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert!(
        store
            .playback("historical", generation, &["uppercase"], &["A"])
            .unwrap()
            .exact
    );
}

#[test]
fn startup_projection_authority_guards_cover_insert_and_update() {
    let db = database();
    db.execute_batch("DROP TRIGGER immutable_projections_insert; DROP TRIGGER immutable_projections_update; DROP TRIGGER immutable_projection_rebuild_status_insert; DROP TRIGGER immutable_projection_rebuild_status_update;").unwrap();
    for table in ["projections", "projection_rebuild_status"] {
        for (n, status) in ["missing", "stale", "drifted", "failed"]
            .into_iter()
            .enumerate()
        {
            let id = format!("{table}-bad-{n}");
            let insert = if table == "projections" {
                db.execute("INSERT INTO projections(id,projection_schema_version,event_history_generation,source_generation,authoritative,status) VALUES(?,1,0,0,1,?)", rusqlite::params![id, status])
            } else {
                db.execute("INSERT INTO projection_rebuild_status(projection_id,status,authoritative) VALUES(?,?,1)", rusqlite::params![id, status])
            };
            assert!(insert.is_ok());
        }
    }
    assert!(Store::from_connection(db).is_err());
}
