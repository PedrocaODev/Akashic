use crate::artifacts;
use artifacts::{ReconciliationInput, Store};

fn store() -> Store {
    Store::open_in_memory().expect("artifact store")
}

#[test]
fn clean_reconciliation_covers_all_durable_surfaces_without_repair() {
    let store = store();
    let result = store.reconcile(ReconciliationInput::matching());
    assert_eq!(result.unwrap().status, "clean");
    assert_eq!(store.reconciliation_count().unwrap(), 1);
}

#[test]
fn reconciliation_records_every_mismatch_and_requires_guarded_resolution() {
    let store = store();
    let result = store.reconcile(ReconciliationInput::mismatching()).unwrap();
    assert_eq!(result.status, "blocked");
    assert_eq!(result.mismatches.len(), 4);
    assert!(!result.repair_applied);
}
