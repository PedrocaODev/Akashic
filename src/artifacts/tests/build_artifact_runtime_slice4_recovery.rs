use crate::artifacts;
use artifacts::{GitObservationAdapter, Import, ReconciliationScope, Store};
use std::fs;
use std::path::{Path, PathBuf};

fn path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("akashic-e2-{label}-{}", std::process::id()))
}

fn accepted(store: &Store, file: &Path) {
    store
        .import(
            Import::new("op", "test", "file", "before")
                .artifact("artifact")
                .file(file),
        )
        .unwrap();
}

#[test]
fn trusted_scope_uses_durable_version_not_live_snapshot() {
    let file = path("tamper");
    fs::write(&file, b"before").unwrap();
    let store = Store::open_in_memory().unwrap();
    accepted(&store, &file);
    fs::write(&file, b"tampered").unwrap();
    let scope = ReconciliationScope::accepted(&store, "op", &file).unwrap();
    let run = store
        .reconcile_owned(scope, [], GitObservationAdapter::not_applicable())
        .unwrap();
    assert_eq!(run.status, "blocked");
    assert_eq!(store.recovery_action_count().unwrap(), 0);
    assert_eq!(store.recovery_outcome_count().unwrap(), 0);
    fs::remove_file(file).unwrap();
}

#[test]
fn missing_or_stale_accepted_operation_is_rejected() {
    let file = path("stale");
    fs::write(&file, b"before").unwrap();
    let store = Store::open_in_memory().unwrap();
    accepted(&store, &file);
    assert!(matches!(
        ReconciliationScope::accepted(&store, "missing", &file),
        Err(artifacts::Error::Conflict)
    ));
    fs::remove_file(file).unwrap();
}
