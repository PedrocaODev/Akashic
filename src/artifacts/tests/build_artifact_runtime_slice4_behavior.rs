use crate::artifacts;
use artifacts::{
    Event, GitObservationAdapter, Import, ReconciliationScope, RecoveryCheckpoint, RecoveryRequest,
    Store,
};
use std::fs;
use std::path::{Path, PathBuf};

fn store() -> Store {
    Store::open_in_memory().expect("artifact store")
}

fn markdown(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "akashic-slice4-behavior-{label}-{}",
        std::process::id()
    ))
}

fn accepted(store: &Store, operation: &str, path: &Path, bytes: &[u8]) -> ReconciliationScope {
    fs::write(path, bytes).unwrap();
    store
        .import(
            Import::new(
                operation,
                "test",
                "file",
                std::str::from_utf8(bytes).unwrap(),
            )
            .artifact("artifact")
            .file(path),
        )
        .unwrap();
    ReconciliationScope::accepted(store, operation, path).unwrap()
}

#[test]
fn store_owned_clean_reconciliation_uses_real_events_projection_file_and_git() {
    let store = store();
    store
        .append_event(Event::new("event-1", 1, "set", "value", "value"))
        .unwrap();
    store.rebuild_projection("projection-1", 1).unwrap();
    let path = markdown("clean");
    let scope = accepted(&store, "clean-op", &path, b"# authored\n");
    let git = GitObservationAdapter::not_applicable();

    let result = store
        .reconcile_owned(scope, ["event-1", "projection-1"], git)
        .unwrap();

    assert_eq!(result.status, "clean");
    assert!(!result.repair_applied);
    assert_eq!(result.provenance("events").expected, "store");
    assert_eq!(result.provenance("projection").observed, "store");
    assert_eq!(result.provenance("filesystem").observed, "filesystem");
    assert_eq!(result.provenance("git").observed, "not_applicable");
    assert_eq!(store.reconciliation_count().unwrap(), 1);
}

#[test]
fn each_surface_mismatch_is_durable_blocked_and_non_authoritative() {
    for changed in ["event", "projection", "filesystem", "git"] {
        let store = store();
        let path = markdown(&format!("mismatch-{changed}"));
        let scope = accepted(&store, &format!("op-{changed}"), &path, b"# authored\n");
        if changed == "filesystem" {
            fs::remove_file(&path).unwrap();
        }
        if changed != "event" {
            store
                .append_event(Event::new("event-1", 1, "set", "value", "value"))
                .unwrap();
        }
        if changed != "projection" {
            store.rebuild_projection("projection-1", 1).unwrap();
        }
        let result = store
            .reconcile_owned(
                scope,
                if changed == "event" {
                    ["event-missing", "projection-1"]
                } else if changed == "projection" {
                    ["event-1", "projection-missing"]
                } else {
                    ["event-1", "projection-1"]
                },
                if changed == "git" {
                    GitObservationAdapter::applicable()
                } else {
                    GitObservationAdapter::not_applicable()
                },
            )
            .unwrap();
        assert_eq!(result.status, "blocked");
        assert!(!result.authoritative);
        assert_eq!(result.mismatches.len(), 1);
        assert_eq!(store.discrepancy_count().unwrap(), 1);
        if changed == "projection" { /* absent identifier is the observed mismatch */ }
        if changed == "filesystem" {
            let _ = fs::remove_file(&path);
        }
    }
}

#[test]
fn guarded_recovery_revalidates_and_is_exactly_once() {
    let store = store();
    let path = markdown("recovery");
    store
        .append_event(Event::new("event-1", 1, "set", "value", "value"))
        .unwrap();
    store.rebuild_projection("projection-1", 1).unwrap();
    let scope = accepted(&store, "recovery-op", &path, b"authored\n");
    fs::remove_file(&path).unwrap();
    let blocked = store
        .reconcile_owned(
            scope,
            ["event-1", "projection-1"],
            GitObservationAdapter::not_applicable(),
        )
        .unwrap();
    assert!(matches!(
        store.recover_blocked(&blocked, RecoveryRequest::unsafe_request("unsafe", &path)),
        Err(artifacts::Error::Conflict)
    ));
    let mut request = RecoveryRequest::safe("restore-recovery-op", &path, b"authored\n");
    request.authorization_identity = "test".into();
    let applied = store
        .recover_uniquely_safe(&blocked, request.clone())
        .unwrap();
    assert_eq!(applied.status, "applied");
    assert_eq!(fs::read(&path).unwrap(), b"authored\n");
    assert_eq!(store.recovery_action_count().unwrap(), 1);
    assert_eq!(store.recovery_outcome_count().unwrap(), 2);
    assert_eq!(
        store.recover_uniquely_safe(&blocked, request).unwrap(),
        applied
    );
    let changed = RecoveryRequest::safe("other-op", &path, b"changed\n");
    assert!(matches!(
        store.recover_uniquely_safe(&blocked, changed),
        Err(artifacts::Error::Conflict)
    ));
}

#[test]
fn uniquely_safe_recovery_never_overwrites_changed_authored_bytes() {
    let store = store();
    let path = markdown("recovery-changed");
    store
        .append_event(Event::new("event-1", 1, "set", "value", "value"))
        .unwrap();
    store.rebuild_projection("projection-1", 1).unwrap();
    let scope = accepted(&store, "recovery-changed-op", &path, b"authored\n");
    fs::write(&path, b"changed\n").unwrap();
    let blocked = store
        .reconcile_owned(
            scope,
            ["event-1", "projection-1"],
            GitObservationAdapter::not_applicable(),
        )
        .unwrap();
    let mut request = RecoveryRequest::safe("recovery-changed-op", &path, b"authored\n");
    request.authorization_identity = "test".into();

    assert!(matches!(
        store.recover_uniquely_safe(&blocked, request.clone()),
        Err(artifacts::Error::Conflict)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"changed\n");
    assert_eq!(store.recovery_action_count().unwrap(), 0);
    assert_eq!(store.recovery_outcome_count().unwrap(), 0);
    assert!(matches!(
        store.recover_uniquely_safe(&blocked, request),
        Err(artifacts::Error::Conflict)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"changed\n");
    assert_eq!(store.recovery_action_count().unwrap(), 0);
    assert_eq!(store.recovery_outcome_count().unwrap(), 0);
}

#[cfg(unix)]
#[test]
fn recovery_rejects_symlinked_parent_before_writing() {
    use std::os::unix::fs::symlink;

    let store = store();
    let real = markdown("symlink-real");
    let link = markdown("symlink-parent");
    fs::create_dir(&real).unwrap();
    symlink(&real, &link).unwrap();
    let path = link.join("authored.md");
    store
        .append_event(Event::new("event-1", 1, "set", "value", "value"))
        .unwrap();
    store.rebuild_projection("projection-1", 1).unwrap();
    let scope = accepted(&store, "symlink-op", &path, b"authored\n");
    fs::remove_file(&path).unwrap();
    let blocked = store
        .reconcile_owned(
            scope,
            ["event-1", "projection-1"],
            GitObservationAdapter::not_applicable(),
        )
        .unwrap();
    let mut request = RecoveryRequest::safe("symlink-op", &path, b"authored\n");
    request.authorization_identity = "test".into();

    assert!(matches!(
        store.recover_uniquely_safe(&blocked, request),
        Err(artifacts::Error::Conflict)
    ));
    assert_eq!(store.recovery_action_count().unwrap(), 0);
    assert_eq!(store.recovery_outcome_count().unwrap(), 0);
    assert!(!path.exists());
    fs::remove_file(&link).unwrap();
    fs::remove_dir(&real).unwrap();
}

#[test]
fn crash_checkpoints_reconcile_without_blind_repeat() {
    for checkpoint in [
        RecoveryCheckpoint::BeforePrepareCommit,
        RecoveryCheckpoint::AfterPrepare,
        RecoveryCheckpoint::AfterEffectBeforeOutcome,
        RecoveryCheckpoint::OutcomeCommitUnknown,
    ] {
        let store = store();
        let path = markdown("crash");
        store
            .append_event(Event::new("event-1", 1, "set", "value", "value"))
            .unwrap();
        store.rebuild_projection("projection-1", 1).unwrap();
        let operation = format!("crash-op-{checkpoint:?}");
        let scope = accepted(&store, &operation, &path, b"new\n");
        fs::remove_file(&path).unwrap();
        let run = store
            .reconcile_owned(
                scope,
                ["event-1", "projection-1"],
                GitObservationAdapter::not_applicable(),
            )
            .unwrap();
        let mut request = RecoveryRequest::safe(&operation, &path, b"new\n");
        request.authorization_identity = "test".into();
        let result = store
            .recover_with_checkpoint(&run, request.clone(), checkpoint)
            .unwrap();
        assert!(matches!(
            result.status.as_str(),
            "blocked" | "unknown" | "applied"
        ));
        let retry = store
            .recover_with_checkpoint(&run, request, checkpoint)
            .unwrap();
        assert_eq!(retry, result);
    }
}
