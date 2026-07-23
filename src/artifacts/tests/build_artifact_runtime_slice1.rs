use crate::artifacts;
use crate::runtime;

use artifacts::{Error, Import, Store};
use rusqlite::Connection;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "akashic-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn replace_store(path: &Path) {
    let old = path.with_extension("old");
    fs::rename(path, &old).unwrap();
    fs::write(path, b"replacement bytes").unwrap();
}

fn replace_target_same_bytes(path: &Path) {
    let replacement = path.with_extension("replacement");
    let old = path.with_extension("old");
    fs::write(&replacement, b"same").unwrap();
    fs::rename(path, &old).unwrap();
    fs::rename(&replacement, path).unwrap();
}

fn add_setuid(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o1600)).unwrap();
}

fn legacy(path: &Path, version: i64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = Connection::open(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    db.execute_batch(&format!(
        "CREATE TABLE schema_metadata(key TEXT PRIMARY KEY,value INTEGER NOT NULL);
         INSERT INTO schema_metadata VALUES('schema_version',{version});
         INSERT INTO schema_metadata VALUES('migration_lineage',0);
         CREATE TABLE artifacts(id TEXT PRIMARY KEY,owner TEXT NOT NULL,source TEXT NOT NULL);
         CREATE TABLE versions(id TEXT PRIMARY KEY,artifact_id TEXT NOT NULL,hash TEXT NOT NULL,
           canonical TEXT NOT NULL,ancestry TEXT,operation TEXT NOT NULL UNIQUE,
           actor TEXT NOT NULL,source TEXT NOT NULL);"
    ))
    .unwrap();
}

fn populated_legacy(path: &Path) {
    legacy(path, 0);
    let db = Connection::open(path).unwrap();
    db.execute(
        "INSERT INTO artifacts VALUES('legacy-artifact','legacy-owner','legacy-source')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO versions(id,artifact_id,hash,canonical,ancestry,operation,actor,source) VALUES('legacy-version','legacy-artifact','53d78b316bb8272d761c0011d62a258618d2a374ee727b786a64671ba58726b3','legacy-body',NULL,'legacy-operation','legacy-owner','legacy-source')",
        [],
    )
    .unwrap();
}

#[test]
fn replacing_file_before_constructing_next_request_is_rejected_without_mutation() {
    let root = temp("durable-lineage");
    let file = root.join("artifact.md");
    fs::write(&file, "body").unwrap();
    let store = Store::open(&root.join("store.sqlite")).unwrap();
    let first = store
        .import(
            Import::new("one", "owner", "source", "body")
                .file(&file)
                .artifact("artifact"),
        )
        .unwrap();
    drop(store);
    let replacement = root.join("replacement.md");
    fs::write(&replacement, "body").unwrap();
    fs::rename(&file, root.join("original.md")).unwrap();
    fs::rename(&replacement, &file).unwrap();
    let reopened = Store::open(&root.join("store.sqlite")).unwrap();
    let outcome = reopened.import(
        Import::new("two", "owner", "source", "body")
            .file(&file)
            .artifact("artifact")
            .ancestry(Some(&first.version_id)),
    );
    if !matches!(outcome, Err(Error::Drift)) {
        panic!("unexpected: {outcome:?}");
    }
    assert_eq!(fs::read(&file).unwrap(), b"body");
    assert_eq!(reopened.version_count().unwrap(), 1);
}

#[test]
fn populated_migration_retries_with_legacy_operation_fingerprint_and_result() {
    let root = temp("migration-retry");
    let path = root.join("legacy.sqlite");
    populated_legacy(&path);
    let store = Store::open(&path).unwrap();
    let request = Import::new(
        "legacy-operation",
        "legacy-owner",
        "legacy-source",
        "legacy-body",
    )
    .artifact("legacy-artifact");
    let result = store.import(request.clone()).unwrap();
    assert_eq!(result.version_id, "legacy-version");
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.import(request).unwrap().version_id,
        "legacy-version"
    );
}

#[test]
fn migrated_legacy_operation_with_file_conflicts_without_adding_a_version() {
    let root = temp("migration-file-conflict");
    let path = root.join("legacy.sqlite");
    populated_legacy(&path);
    let file = root.join("legacy.md");
    fs::write(&file, "legacy-body").unwrap();
    let store = Store::open(&path).unwrap();
    assert!(matches!(
        store.import(
            Import::new(
                "legacy-operation",
                "legacy-owner",
                "legacy-source",
                "legacy-body",
            )
            .artifact("legacy-artifact")
            .file(&file),
        ),
        Err(Error::Conflict)
    ));
    assert_eq!(store.version_count().unwrap(), 1);
}

#[test]
fn identical_file_content_under_different_logical_paths_conflicts() {
    let root = temp("logical-path-fingerprint");
    let first = root.join("first.md");
    let second = root.join("second.md");
    fs::write(&first, "same").unwrap();
    fs::write(&second, "same").unwrap();
    let store = Store::open_in_memory().unwrap();
    store
        .import(
            Import::new("op", "owner", "source", "same")
                .artifact("artifact")
                .file(&first),
        )
        .unwrap();
    assert!(matches!(
        store.import(
            Import::new("op", "owner", "source", "same")
                .artifact("artifact")
                .file(&second),
        ),
        Err(Error::Conflict)
    ));
    assert_eq!(store.version_count().unwrap(), 1);
}

#[test]
fn accepted_import_persists_file_and_content_only_observations() {
    let root = temp("observation-binding");
    let file = root.join("nested").join("artifact.md");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "same").unwrap();
    let store = Store::open_in_memory().unwrap();
    store
        .import(
            Import::new("file", "owner", "source", "same")
                .file(&file)
                .artifact("file-artifact"),
        )
        .unwrap();
    let mut expected = file
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .into_os_string()
        .into_vec();
    expected.extend_from_slice(b"/artifact.md");
    let observation = store.version_observation("file-artifact").unwrap().unwrap();
    assert_eq!(observation.0, Some(expected));
    assert!(observation
        .1
        .as_ref()
        .is_some_and(|identity| !identity.is_empty()));
    assert_eq!(observation.2, Some("not_applicable".to_owned()));
    assert_eq!(
        (
            observation.3,
            observation.4,
            observation.5,
            observation.6,
            observation.7
        ),
        (None, None, None, None, None)
    );

    store
        .import(Import::new("content", "owner", "source", "same").artifact("content-artifact"))
        .unwrap();
    let content = store
        .version_observation("content-artifact")
        .unwrap()
        .unwrap();
    assert!(content.0.is_none());
    assert!(content.1.is_none());
    assert_eq!(
        (content.2, content.3, content.4, content.5, content.6, content.7),
        (None, None, None, None, None, None)
    );
}

#[test]
fn file_binding_is_stable_across_valid_ancestry_and_rejects_aliases_and_content_only() {
    let root = temp("stable-binding");
    let a = root.join("a.md");
    let b = root.join("b.md");
    fs::write(&a, "same").unwrap();
    fs::write(&b, "same").unwrap();
    let store = Store::open_in_memory().unwrap();
    let first = store
        .import(
            Import::new("one", "owner", "source", "same")
                .file(&a)
                .artifact("artifact"),
        )
        .unwrap();
    assert!(store
        .import(
            Import::new("two", "owner", "source", "same")
                .file(&a)
                .artifact("artifact")
                .ancestry(Some(&first.version_id))
        )
        .is_ok());
    assert!(matches!(
        store.import(
            Import::new("three", "owner", "source", "same")
                .file(&b)
                .artifact("artifact")
                .ancestry(Some(&first.version_id))
        ),
        Err(Error::Conflict)
    ));
    let alias = root.join("alias.md");
    fs::hard_link(&a, &alias).unwrap();
    assert!(matches!(
        store.import(
            Import::new("four", "owner", "source", "same")
                .file(&alias)
                .artifact("artifact")
                .ancestry(Some(&first.version_id))
        ),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store.import(
            Import::new("five", "owner", "source", "same")
                .artifact("artifact")
                .ancestry(Some(&first.version_id))
        ),
        Err(Error::Conflict)
    ));
    assert_eq!(store.version_count().unwrap(), 2);
}

#[test]
fn precommit_reobservation_rejects_same_bytes_on_new_inode_without_rows() {
    let root = temp("precommit-reobserve");
    let file = root.join("artifact.md");
    fs::write(&file, "same").unwrap();
    let store = Store::open_in_memory().unwrap();
    let _guard = artifacts::replace_before_sqlite_open(replace_target_same_bytes);
    assert!(matches!(
        store.import(
            Import::new("replace", "owner", "source", "same")
                .file(&file)
                .artifact("artifact")
        ),
        Err(Error::Drift)
    ));
    assert_eq!(store.version_count().unwrap(), 0);
    assert_eq!(store.accepted_record_count("replace").unwrap(), 0);
}

#[test]
fn known_hash_and_explicit_file_import_are_enforced() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(
        store
            .import(Import::new("hash", "owner", "source", "hello"))
            .unwrap()
            .hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    let root = temp("explicit-discovery");
    let file = root.join("artifact.md");
    fs::write(&file, "body").unwrap();
    assert!(matches!(
        store.import(Import::new("implicit", "owner", "source", "body").file(&file)),
        Err(Error::Conflict)
    ));
}

#[test]
fn canonical_bytes_hash_and_distinct_operations_preserve_provenance() {
    let store = Store::open_in_memory().unwrap();
    let content = "# title\r\n\u{00e9}";
    let first = store
        .import(Import::new("op-a", "alice", "fixture.md", content))
        .unwrap();
    let second = store
        .import(
            Import::new("op-b", "alice", "fixture.md", content)
                .artifact("fixture.md")
                .ancestry(Some(&first.version_id)),
        )
        .unwrap();
    assert_eq!(first.canonical.as_bytes(), content.as_bytes());
    assert_ne!(first.version_id, second.version_id);
    assert_eq!(store.version_count().unwrap(), 2);
}

#[test]
fn accepted_operation_is_idempotent_but_changed_fingerprint_is_rejected() {
    let store = Store::open_in_memory().unwrap();
    let request = Import::new("op", "alice", "source", "one");
    let original = store.import(request.clone()).unwrap();
    assert_eq!(store.import(request).unwrap(), original);
    assert!(matches!(
        store.import(Import::new("op", "bob", "source", "one")),
        Err(Error::Conflict)
    ));
    assert_eq!(store.discrepancy_count().unwrap(), 1);
}

#[test]
fn expected_hash_file_identity_and_read_boundaries_are_rejected() {
    let root = temp("drift");
    let file = root.join("artifact.md");
    fs::write(&file, "observed").unwrap();
    let store = Store::open_in_memory().unwrap();
    assert!(matches!(
        store.import(Import::new("wrong-hash", "a", "s", "content").expected_hash("bad")),
        Err(Error::Drift)
    ));
    assert!(matches!(
        store.import(
            Import::new("changed", "a", "s", "content")
                .file(&file)
                .artifact("file")
        ),
        Err(Error::Drift)
    ));
    let link = root.join("link.md");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert!(matches!(
        store.import(
            Import::new("symlink", "a", "s", "observed")
                .file(&link)
                .artifact("link")
        ),
        Err(Error::Drift)
    ));
    assert!(matches!(
        store.import(
            Import::new("missing", "a", "s", "x")
                .file(&root.join("missing"))
                .artifact("missing")
        ),
        Err(Error::Drift)
    ));
    assert!(store.discrepancy_count().unwrap() >= 4);
}

#[test]
fn file_replacement_with_same_content_is_rejected_by_expected_identity() {
    let root = temp("replacement");
    let file = root.join("artifact.md");
    fs::write(&file, "same").unwrap();
    let request = Import::new("replace", "a", "s", "same")
        .file(&file)
        .artifact("artifact");
    let replacement = root.join("replacement.md");
    fs::rename(&file, &replacement).unwrap();
    fs::write(&file, "same").unwrap();
    assert!(matches!(
        Store::open_in_memory().unwrap().import(request),
        Err(Error::Drift)
    ));
}

#[test]
fn ancestry_and_ownership_conflicts_are_recorded() {
    let store = Store::open_in_memory().unwrap();
    let first = store
        .import(Import::new("one", "alice", "source", "one").artifact("id"))
        .unwrap();
    assert!(matches!(
        store.import(Import::new("two", "alice", "source", "two").artifact("id")),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store.import(Import::new("three", "bob", "source", "three").artifact("id")),
        Err(Error::Conflict)
    ));
    assert!(store
        .import(
            Import::new("child", "alice", "source", "two")
                .artifact("id")
                .ancestry(Some(&first.version_id))
        )
        .is_ok());
    assert!(store.discrepancy_count().unwrap() >= 2);
    let discrepancy = store.discrepancy("three").unwrap().unwrap();
    assert_eq!(discrepancy.actor, "bob");
    assert_eq!(discrepancy.source, "source");
    assert_eq!(discrepancy.observed_owner.as_deref(), Some("alice"));
    assert_eq!(discrepancy.observed_source.as_deref(), Some("source"));
    assert_eq!(
        discrepancy.observed_ancestry.as_deref(),
        Some(first.version_id.as_str())
    );
}

#[test]
fn current_schema_rejects_missing_relational_contract_and_disabled_immutability() {
    let root = temp("schema-contract");
    let path = root.join("store.sqlite");
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER immutable_versions_update; CREATE TRIGGER immutable_versions_update BEFORE UPDATE ON versions WHEN 0 BEGIN SELECT RAISE(ABORT,'immutable'); END;")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));

    let path = root.join("fk.sqlite");
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; ALTER TABLE operations RENAME TO old_operations; CREATE TABLE operations(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,result TEXT NOT NULL,version_id TEXT,lineage TEXT,rejection_code TEXT,schema_version INTEGER NOT NULL); INSERT INTO operations SELECT * FROM old_operations; DROP TABLE old_operations;")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn current_schema_rejects_accepted_rows_without_exact_operation_and_lineage_agreement() {
    let root = temp("accepted-contract");
    let path = root.join("store.sqlite");
    let store = Store::open(&path).unwrap();
    let version = store
        .import(Import::new("op", "owner", "source", "body"))
        .unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "DROP TRIGGER immutable_lineage_delete; DELETE FROM lineage WHERE operation_id='op';",
    )
    .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    assert!(!version.version_id.is_empty());
}

#[test]
fn current_schema_rejects_orphan_accepted_operation_and_version_rows() {
    let root = temp("accepted-orphans");
    let operation_path = root.join("operation.sqlite");
    let store = Store::open(&operation_path).unwrap();
    store
        .import(Import::new("op", "owner", "source", "body"))
        .unwrap();
    drop(store);
    let db = Connection::open(&operation_path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER immutable_operations_delete; DROP TRIGGER immutable_lineage_delete; DELETE FROM lineage WHERE operation_id='op'; DELETE FROM operations WHERE id='op';").unwrap();
    db.execute("INSERT INTO operations(id,fingerprint,result,version_id,lineage,rejection_code,schema_version) VALUES('op','fp','accepted','v-missing',NULL,NULL,1)", []).unwrap();
    drop(db);
    assert!(matches!(
        Store::open(&operation_path),
        Err(Error::AmbiguousSchema)
    ));

    let version_path = root.join("version.sqlite");
    let store = Store::open(&version_path).unwrap();
    store
        .import(Import::new("op", "owner", "source", "body"))
        .unwrap();
    drop(store);
    let db = Connection::open(&version_path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER immutable_operations_delete; DROP TRIGGER immutable_lineage_delete; DELETE FROM lineage WHERE operation_id='op'; DELETE FROM operations WHERE id='op';").unwrap();
    db.execute("INSERT INTO operations(id,fingerprint,result,version_id,lineage,rejection_code,schema_version) VALUES('other','fp','accepted',(SELECT id FROM versions WHERE operation='op'),NULL,NULL,1)", []).unwrap();
    drop(db);
    assert!(matches!(
        Store::open(&version_path),
        Err(Error::AmbiguousSchema)
    ));
}

#[test]
fn current_schema_rejects_malformed_operation_result_values() {
    let root = temp("malformed-result");
    let path = root.join("store.sqlite");
    Store::open(&path)
        .unwrap()
        .import(Import::new("op", "owner", "source", "body"))
        .unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER immutable_operations_update; UPDATE operations SET result='unknown' WHERE id='op';")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn current_schema_rejects_orphan_rejected_rows_and_bad_discrepancy_links() {
    let root = temp("malformed-rejected");
    let path = root.join("store.sqlite");
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER immutable_operations_delete; DROP TRIGGER immutable_discrepancies_delete;
        INSERT INTO operations(id,fingerprint,result,version_id,lineage,rejection_code,schema_version) VALUES('orphan','fp','rejected',NULL,NULL,'drift',1);")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));

    let path = root.join("bad-link.sqlite");
    let store = Store::open(&path).unwrap();
    assert!(matches!(
        store.import(Import::new("reject", "owner", "source", "body").expected_hash("wrong")),
        Err(Error::Drift)
    ));
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER immutable_discrepancies_update; UPDATE discrepancies SET operation_result_id='orphan' WHERE operation='reject';")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn legacy_shape_rejects_type_and_constraint_changes_before_migration() {
    let root = temp("legacy-shape");
    let path = root.join("legacy.sqlite");
    legacy(&path, 0);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE versions RENAME TO old_versions; CREATE TABLE versions(id TEXT PRIMARY KEY,artifact_id TEXT NOT NULL,hash BLOB NOT NULL,canonical TEXT NOT NULL,ancestry TEXT,operation TEXT NOT NULL UNIQUE,actor TEXT NOT NULL,source TEXT NOT NULL); INSERT INTO versions SELECT * FROM old_versions; DROP TABLE old_versions;")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn legacy_shape_rejects_missing_operation_uniqueness_and_added_foreign_keys() {
    let root = temp("legacy-contract");
    let path = root.join("legacy.sqlite");
    legacy(&path, 0);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE versions RENAME TO old_versions; CREATE TABLE versions(id TEXT PRIMARY KEY,artifact_id TEXT NOT NULL,hash TEXT NOT NULL,canonical TEXT NOT NULL,ancestry TEXT,operation TEXT NOT NULL,actor TEXT NOT NULL,source TEXT NOT NULL,FOREIGN KEY(artifact_id) REFERENCES artifacts(id)); INSERT INTO versions SELECT * FROM old_versions; DROP TABLE old_versions;")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn legacy_shape_rejects_composite_operation_uniqueness_even_when_empty() {
    let root = temp("legacy-composite-unique");
    let path = root.join("legacy.sqlite");
    legacy(&path, 0);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE versions RENAME TO old_versions; CREATE TABLE versions(id TEXT PRIMARY KEY,artifact_id TEXT NOT NULL,hash TEXT NOT NULL,canonical TEXT NOT NULL,ancestry TEXT,operation TEXT NOT NULL,actor TEXT NOT NULL,source TEXT NOT NULL,UNIQUE(operation,actor)); INSERT INTO versions SELECT * FROM old_versions; DROP TABLE old_versions;").unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn fresh_schema_reopens_and_legacy_fixture_migrates_transactionally() {
    let root = temp("schema");
    let path = root.join("artifacts.sqlite");
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 3);
    }
    assert_eq!(Store::open(&path).unwrap().schema_version().unwrap(), 3);

    let legacy_path = root.join("legacy.sqlite");
    legacy(&legacy_path, 0);
    let migrated = Store::open(&legacy_path).unwrap();
    assert_eq!(migrated.schema_version().unwrap(), 3);
    assert_eq!(migrated.version_count().unwrap(), 0);
    assert!(Store::open(&legacy_path).is_ok());

    let populated_path = root.join("populated-legacy.sqlite");
    populated_legacy(&populated_path);
    let migrated = Store::open(&populated_path).unwrap();
    assert_eq!(migrated.version_count().unwrap(), 1);
    assert_eq!(
        migrated.operation_lineage("legacy-operation").unwrap(),
        Some(None)
    );
}

#[test]
fn schema_shape_and_accepted_lineage_are_immutable() {
    let root = temp("shape");
    let path = root.join("store.sqlite");
    let store = Store::open(&path).unwrap();
    let version = store
        .import(Import::new("op", "owner", "source", "body"))
        .unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    for object in ["operations", "discrepancies", "lineage"] {
        assert!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name=?",
                [object],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
                > 0,
            "missing {object}"
        );
    }
    assert!(db
        .execute("UPDATE operations SET result='changed' WHERE id='op'", [])
        .is_err());
    assert!(db
        .execute("DELETE FROM operations WHERE id='op'", [])
        .is_err());
    assert!(db
        .execute(
            "UPDATE versions SET canonical='changed' WHERE id=?",
            [&version.version_id]
        )
        .is_err());
    assert!(db
        .execute("DELETE FROM versions WHERE id=?", [&version.version_id])
        .is_err());
}

#[test]
fn artifact_identity_and_operation_lineage_are_distinct_and_conflicts_reject() {
    let store = Store::open_in_memory().unwrap();
    let first = store
        .import(Import::new("one", "alice", "source", "one").artifact("stable"))
        .unwrap();
    assert_eq!(first.artifact_id, "stable");
    assert_eq!(store.operation_lineage("one").unwrap(), Some(None));
    assert!(matches!(
        store.import(Import::new("two", "bob", "source", "two").artifact("stable")),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store.import(Import::new("three", "alice", "other", "three").artifact("stable")),
        Err(Error::Conflict)
    ));
}

#[test]
fn rejected_retry_replays_original_classification_without_new_records() {
    let store = Store::open_in_memory().unwrap();
    let request = Import::new("reject", "a", "s", "body").expected_hash("wrong");
    assert!(matches!(store.import(request.clone()), Err(Error::Drift)));
    let discrepancies = store.discrepancy_count().unwrap();
    assert!(matches!(store.import(request), Err(Error::Drift)));
    assert_eq!(store.discrepancy_count().unwrap(), discrepancies);
    assert!(matches!(
        store.import(Import::new("reject", "b", "s", "body").expected_hash("wrong")),
        Err(Error::Conflict)
    ));
}

#[test]
fn malformed_future_and_partial_schema_boundaries_fail_without_partial_migration() {
    let root = temp("schema-boundaries");
    let future = root.join("future.sqlite");
    legacy(&future, 99);
    assert!(matches!(
        Store::open(&future),
        Err(Error::UnsupportedSchema(99))
    ));

    let partial = root.join("partial.sqlite");
    let db = Connection::open(&partial).unwrap();
    db.execute(
        "CREATE TABLE schema_metadata(key TEXT PRIMARY KEY,value INTEGER NOT NULL)",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO schema_metadata VALUES('schema_version',0)", [])
        .unwrap();
    assert!(matches!(Store::open(&partial), Err(Error::AmbiguousSchema)));
    assert!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='discrepancies'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap()
            == 0
    );
}

#[test]
fn migration_rejects_bad_legacy_hash_and_records_source_lineage() {
    let root = temp("legacy-validation");
    let path = root.join("bad.sqlite");
    populated_legacy(&path);
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE versions SET hash='wrong'", []).unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    let db = Connection::open(&path).unwrap();
    assert!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='legacy_versions'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap()
            == 0
    );

    let good = root.join("good.sqlite");
    populated_legacy(&good);
    let store = Store::open(&good).unwrap();
    let db = Connection::open(&good).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT source_schema_version,source_lineage FROM versions WHERE id='legacy-version'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        (0, "legacy".to_owned())
    );
    assert_eq!(db.query_row("SELECT source_schema_version,target_schema_version,operation,actor,source,outcome FROM compatibility_decisions", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))).unwrap(), (0, 1, "migration".to_owned(), "system".to_owned(), "artifact-store".to_owned(), "accepted".to_owned()));
    assert_eq!(
        store.operation_lineage("legacy-operation").unwrap(),
        Some(None)
    );
}

#[test]
fn fresh_and_migrated_schema_objects_are_equivalent_and_unknown_objects_reject() {
    let root = temp("schema-equivalence");
    let fresh = root.join("fresh.sqlite");
    Store::open(&fresh).unwrap();
    let legacy_path = root.join("legacy.sqlite");
    legacy(&legacy_path, 0);
    Store::open(&legacy_path).unwrap();
    let a = Connection::open(&fresh).unwrap();
    let b = Connection::open(&legacy_path).unwrap();
    let objects = |db: &Connection| {
        let mut objects: Vec<String> = db
            .prepare("SELECT type||':'||name||':'||ifnull(sql,'') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        objects.sort();
        objects
    };
    assert_eq!(objects(&a), objects(&b));
    b.execute("CREATE TABLE unknown(x TEXT)", []).unwrap();
    assert!(matches!(
        Store::open(&legacy_path),
        Err(Error::AmbiguousSchema)
    ));
}

#[test]
fn discrepancy_retrieval_contains_expected_and_observed_values_and_retries_identically() {
    let store = Store::open_in_memory().unwrap();
    let request = Import::new("observe", "owner", "source", "body").expected_hash("wrong");
    assert!(matches!(store.import(request.clone()), Err(Error::Drift)));
    let first = store.discrepancy("observe").unwrap().unwrap();
    assert_eq!(first.expected_hash.as_deref(), Some("wrong"));
    assert_eq!(first.status, "rejected");
    assert!(matches!(store.import(request), Err(Error::Drift)));
    assert_eq!(store.discrepancy("observe").unwrap().unwrap().id, first.id);
}

#[test]
fn final_database_symlink_is_rejected_before_open() {
    let root = temp("db-safety");
    let target = root.join("target.sqlite");
    Store::open(&target).unwrap();
    let link = root.join("link.sqlite");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(matches!(Store::open(&link), Err(Error::AmbiguousSchema)));
}

#[test]
fn unsafe_existing_regular_database_is_rejected_without_mutation() {
    let root = temp("unsafe-existing-db");
    let path = root.join("store.sqlite");
    Store::open(&path).unwrap();
    drop(Connection::open(&path).unwrap());
    let before = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
}

#[test]
fn daemon_state_fallbacks_and_security_classification_are_exercised() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = temp("runtime");
    std::env::remove_var("XDG_RUNTIME_DIR");
    std::env::set_var("XDG_STATE_HOME", root.join("state"));
    std::env::set_var("HOME", root.join("home"));
    let paths = runtime::probe().unwrap();
    assert!(paths.socket.starts_with(root.join("state")));
    assert!(runtime::artifact_store_path().is_ok());
    std::env::remove_var("XDG_STATE_HOME");
    assert!(runtime::probe().is_ok());
    std::env::set_var("XDG_STATE_HOME", root.join("unsafe"));
    fs::create_dir_all(root.join("unsafe")).unwrap();
    fs::set_permissions(root.join("unsafe"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        runtime::probe(),
        Err(runtime::RuntimeError::Unsafe)
    ));
}

#[test]
fn legacy_shape_and_values_are_compatibility_failures() {
    let root = temp("legacy-shape");
    let extra = root.join("extra.sqlite");
    legacy(&extra, 0);
    Connection::open(&extra)
        .unwrap()
        .execute("CREATE TABLE extra(value TEXT)", [])
        .unwrap();
    assert!(matches!(Store::open(&extra), Err(Error::AmbiguousSchema)));

    let malformed = root.join("malformed.sqlite");
    legacy(&malformed, 0);
    Connection::open(&malformed)
        .unwrap()
        .execute("INSERT INTO schema_metadata VALUES('unknown',0)", [])
        .unwrap();
    assert!(matches!(
        Store::open(&malformed),
        Err(Error::AmbiguousSchema)
    ));
}

#[test]
fn current_schema_rejects_extra_columns() {
    let root = temp("current-shape");
    let path = root.join("store.sqlite");
    Store::open(&path).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute("ALTER TABLE artifacts ADD COLUMN extra TEXT", [])
        .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn current_schema_rejects_corrupt_hash_and_ancestry_before_use() {
    let root = temp("current-values");
    let path = root.join("store.sqlite");
    let store = Store::open(&path).unwrap();
    let parent = store
        .import(Import::new("parent", "owner", "source", "one").artifact("a"))
        .unwrap();
    store
        .import(
            Import::new("child", "owner", "source", "two")
                .artifact("a")
                .ancestry(Some(&parent.version_id)),
        )
        .unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER immutable_versions_update;")
        .unwrap();
    db.execute(
        "UPDATE versions SET hash='corrupt' WHERE id=?",
        [&parent.version_id],
    )
    .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));

    let path = root.join("ancestry.sqlite");
    let store = Store::open(&path).unwrap();
    let a = store
        .import(Import::new("a", "owner", "source", "a").artifact("a"))
        .unwrap();
    store
        .import(Import::new("b", "owner", "source", "b").artifact("b"))
        .unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER immutable_versions_update;")
        .unwrap();
    db.execute(
        "UPDATE versions SET artifact_id='b' WHERE id=?",
        [a.version_id],
    )
    .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn current_artifacts_and_lineage_carry_schema_versions() {
    // The persisted records, not only metadata, identify their schema.
    let root = temp("record-schema");
    let path = root.join("store.sqlite");
    Store::open(&path)
        .unwrap()
        .import(Import::new("op", "owner", "source", "body"))
        .unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT schema_version FROM artifacts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT schema_version FROM lineage", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn same_inode_durable_file_edit_is_checked_before_proposed_content() {
    let root = temp("same-inode");
    let file = root.join("artifact.md");
    fs::write(&file, "one").unwrap();
    let store = Store::open_in_memory().unwrap();
    let first = store
        .import(
            Import::new("one", "owner", "source", "one")
                .file(&file)
                .artifact("a"),
        )
        .unwrap();
    fs::write(&file, "two").unwrap();
    assert!(matches!(
        store.import(
            Import::new("two", "owner", "source", "two")
                .file(&file)
                .artifact("a")
                .ancestry(Some(&first.version_id))
        ),
        Err(Error::Drift)
    ));
    let discrepancy = store.discrepancy("two").unwrap().unwrap();
    assert_eq!(discrepancy.expected_hash, Some(first.hash.clone()));
    assert!(discrepancy.observed_hash.is_some());
    assert_ne!(discrepancy.observed_hash, Some(first.hash));
}

#[test]
fn altered_retry_of_accepted_operation_reopens_with_original_result_link() {
    let root = temp("altered-accepted-retry");
    let path = root.join("store.sqlite");
    let store = Store::open(&path).unwrap();
    let accepted = store
        .import(Import::new("op", "owner", "source", "one"))
        .unwrap();
    assert!(matches!(
        store.import(Import::new("op", "owner", "source", "two")),
        Err(Error::Conflict)
    ));
    let discrepancy = store.discrepancy("op").unwrap().unwrap();
    assert_eq!(discrepancy.status, "rejected");
    assert_eq!(discrepancy.result, accepted.version_id);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.discrepancy("op").unwrap().unwrap(), discrepancy);
    assert_eq!(
        reopened
            .import(Import::new("op", "owner", "source", "one"))
            .unwrap(),
        accepted
    );
    assert_eq!(reopened.discrepancy_count().unwrap(), 1);
}

#[test]
fn unsafe_create_failure_is_classified_before_sqlite_open() {
    let root = temp("create-failure");
    let directory = root.join("database.sqlite");
    fs::create_dir(&directory).unwrap();
    assert!(matches!(
        Store::open(&directory),
        Err(Error::AmbiguousSchema)
    ));
}

#[test]
fn missing_store_is_created_owner_only_and_replacement_is_rejected_before_schema() {
    let root = temp("secure-open");
    let path = root.join("store.sqlite");
    Store::open(&path).unwrap();
    assert_eq!(fs::symlink_metadata(&path).unwrap().mode() & 0o777, 0o600);

    let path = root.join("replaced.sqlite");
    let _guard = artifacts::replace_before_sqlite_open(replace_store);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    assert_eq!(fs::read(&path).unwrap(), b"replacement bytes");
}

#[test]
fn hostile_store_objects_and_modes_are_rejected_without_mutation() {
    let root = temp("hostile-objects");
    let target = root.join("target");
    fs::write(&target, b"bytes").unwrap();
    for mode in [0o1600, 0o2600, 0o4600, 0o700] {
        fs::set_permissions(&target, fs::Permissions::from_mode(mode)).unwrap();
        let before = fs::read(&target).unwrap();
        assert!(matches!(Store::open(&target), Err(Error::AmbiguousSchema)));
        assert_eq!(fs::read(&target).unwrap(), before);
        assert_eq!(fs::symlink_metadata(&target).unwrap().mode() & 0o7777, mode);
    }

    let directory = root.join("directory");
    fs::create_dir(&directory).unwrap();
    assert!(matches!(
        Store::open(&directory),
        Err(Error::AmbiguousSchema)
    ));
    let socket = root.join("socket");
    let _listener = UnixListener::bind(&socket).unwrap();
    assert!(matches!(Store::open(&socket), Err(Error::AmbiguousSchema)));

    let real_parent = root.join("real-parent");
    let linked_parent = root.join("linked-parent");
    fs::create_dir(&real_parent).unwrap();
    std::os::unix::fs::symlink(&real_parent, &linked_parent).unwrap();
    let nested = linked_parent.join("store.sqlite");
    assert!(matches!(Store::open(&nested), Err(Error::AmbiguousSchema)));
}

#[test]
fn metadata_change_after_precheck_is_rejected_before_schema_sql() {
    let root = temp("metadata-change");
    let path = root.join("store.sqlite");
    let _guard = artifacts::replace_before_sqlite_open(add_setuid);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    assert_eq!(fs::symlink_metadata(&path).unwrap().mode() & 0o7777, 0o1600);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='schema_metadata'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn metadata_validator_uses_effective_uid_and_exact_identity_fields() {
    let root = temp("metadata-validator");
    let path = root.join("store.sqlite");
    fs::write(&path, b"bytes").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let meta = fs::symlink_metadata(&path).unwrap();
    assert_eq!(artifacts::current_uid_for_test(), meta.uid());
    assert!(artifacts::validate_store_metadata_for_test(&meta, meta.uid()).is_ok());
    assert!(
        artifacts::validate_store_metadata_for_test(&meta, meta.uid().saturating_add(1)).is_err()
    );
}

#[test]
fn bundled_sqlite_supports_nofollow() {
    assert!(rusqlite::version_number() >= 3_031_000);
}

#[test]
fn schema_validation_checks_constraints_and_trigger_behavior() {
    let root = temp("strict-schema");
    let path = root.join("store.sqlite");
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE artifacts RENAME TO artifacts_old; CREATE TABLE artifacts(id BOGUS PRIMARY KEY,owner TEXT,source TEXT); INSERT INTO artifacts(id,owner,source) SELECT id,owner,source FROM artifacts_old; DROP TABLE artifacts_old;")
        .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));

    let trigger_path = root.join("trigger.sqlite");
    Store::open(&trigger_path).unwrap();
    let db = Connection::open(&trigger_path).unwrap();
    db.execute_batch("DROP TRIGGER immutable_versions_update; CREATE TRIGGER immutable_versions_update BEFORE UPDATE ON versions BEGIN SELECT 1; END;")
        .unwrap();
    assert!(matches!(
        Store::open(&trigger_path),
        Err(Error::AmbiguousSchema)
    ));
}

#[test]
fn schema_validation_rejects_missing_fk_and_wrong_index() {
    let root = temp("schema-relations");
    let path = root.join("fk.sqlite");
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let version_triggers = {
        let mut statement = db
            .prepare("SELECT name, sql FROM sqlite_master WHERE type='trigger' AND sql LIKE '%versions%'")
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .map(|trigger| trigger.unwrap())
            .collect::<Vec<_>>()
    };
    for (name, _) in &version_triggers {
        db.execute(&format!("DROP TRIGGER {name}"), []).unwrap();
    }
    db.execute_batch("PRAGMA foreign_keys=OFF; CREATE TABLE versions_copy AS SELECT * FROM versions; DROP TABLE versions; ALTER TABLE versions_copy RENAME TO versions;")
        .unwrap();
    for (_, sql) in &version_triggers {
        db.execute_batch(sql).unwrap();
    }
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));

    let path = root.join("index.sqlite");
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "DROP INDEX versions_artifact; CREATE UNIQUE INDEX versions_artifact ON versions(hash);",
    )
    .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn malformed_schema_metadata_type_is_ambiguous_on_open() {
    let root = temp("metadata-type");
    let path = root.join("malformed.sqlite");
    Store::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE schema_metadata SET value='not-an-integer' WHERE key='schema_version'",
        [],
    )
    .unwrap();
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
}

#[test]
fn discrepancy_records_candidate_file_and_durable_values() {
    let root = temp("discrepancy-values");
    let file = root.join("artifact.md");
    fs::write(&file, "one").unwrap();
    let store = Store::open_in_memory().unwrap();
    let first = store
        .import(
            Import::new("parent", "owner", "source", "one")
                .file(&file)
                .artifact("a"),
        )
        .unwrap();
    fs::write(&file, "two").unwrap();
    assert!(matches!(
        store.import(
            Import::new("child", "owner", "source", "two")
                .file(&file)
                .artifact("a")
                .ancestry(Some(&first.version_id))
        ),
        Err(Error::Drift)
    ));
    let d = store.discrepancy("child").unwrap().unwrap();
    assert_eq!(d.expected_hash, Some(first.hash));
    assert!(d.observed_hash.is_some());
    assert_eq!(store.discrepancy("child").unwrap().unwrap().id, d.id);
}

#[test]
fn rejected_ancestry_is_null_and_reopens_without_ambiguity() {
    let root = temp("rejected-ancestry");
    let path = root.join("store.sqlite");
    let store = Store::open(&path).unwrap();
    let first = store
        .import(Import::new("root", "owner", "source", "one"))
        .unwrap();
    assert!(matches!(
        store.import(Import::new("child", "owner", "source", "two").ancestry(Some("wrong"))),
        Err(Error::Conflict)
    ));
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.operation_lineage("child").unwrap(), Some(None));
    assert!(reopened.discrepancy("child").unwrap().is_some());
    assert!(!first.version_id.is_empty());
}

#[test]
fn current_schema_rejects_bad_schema_and_compatibility_values() {
    let root = temp("bad-values");
    for (name, sql) in [
        ("artifact", "UPDATE artifacts SET schema_version=0"),
        ("version", "UPDATE versions SET source_schema_version=2"),
        ("lineage", "UPDATE lineage SET schema_version=0"),
        ("discrepancy", "UPDATE discrepancies SET schema_version=0"),
        (
            "compatibility",
            "UPDATE compatibility_decisions SET outcome='unknown'",
        ),
    ] {
        let path = root.join(format!("{name}.sqlite"));
        let store = Store::open(&path).unwrap();
        if name == "discrepancy" {
            assert!(matches!(
                store.import(Import::new("op", "owner", "source", "body").expected_hash("bad")),
                Err(Error::Drift)
            ));
        } else {
            store
                .import(Import::new("op", "owner", "source", "body"))
                .unwrap();
        }
        if name == "compatibility" {
            let db = Connection::open(&path).unwrap();
            db.execute("INSERT INTO compatibility_decisions(source_schema_version,target_schema_version,operation,actor,source,outcome,result,scope_id,fingerprint_version,fingerprint,created_at) VALUES(0,1,'migration','system','source','accepted','migrated','source',1,'bad','2026-01-01T00:00:00Z')", []).unwrap();
        }
        drop(store);
        let db = Connection::open(&path).unwrap();
        db.execute_batch("DROP TRIGGER immutable_artifacts_update;")
            .ok();
        db.execute_batch(sql).unwrap_or_else(|_| {
            db.execute_batch("DROP TRIGGER immutable_versions_update; DROP TRIGGER immutable_lineage_update; DROP TRIGGER immutable_discrepancies_update;").unwrap();
            db.execute_batch(sql).unwrap();
        });
        drop(db);
        assert!(
            matches!(Store::open(&path), Err(Error::AmbiguousSchema)),
            "{name}"
        );
    }
}

#[test]
fn legacy_migration_rejects_cross_artifact_ancestry() {
    let root = temp("legacy-cross-artifact");
    let path = root.join("legacy.sqlite");
    legacy(&path, 0);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("INSERT INTO artifacts VALUES('a','owner','source'),('b','owner','source'); INSERT INTO versions VALUES('parent','a','ca','parent',NULL,'parent-op','owner','source'); INSERT INTO versions VALUES('child','b','cb','child','parent','child-op','owner','source');")
        .unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err(Error::AmbiguousSchema)));
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='legacy_versions'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn missing_durable_file_identity_is_drift_not_sql() {
    let root = temp("missing-durable-identity");
    let file = root.join("artifact.md");
    fs::write(&file, "body").unwrap();
    let store = Store::open_in_memory().unwrap();
    let first = store
        .import(Import::new("parent", "owner", "source", "body").artifact("artifact"))
        .unwrap();
    assert!(matches!(
        store.import(
            Import::new("child", "owner", "source", "body")
                .file(&file)
                .artifact("artifact")
                .ancestry(Some(&first.version_id))
        ),
        Err(Error::Drift)
    ));
}

#[test]
fn equivalent_resolved_artifact_retry_is_idempotent() {
    let store = Store::open_in_memory().unwrap();
    let first = store
        .import(Import::new("same", "owner", "source", "body"))
        .unwrap();
    assert_eq!(
        store
            .import(Import::new("same", "owner", "source", "body").artifact("source"))
            .unwrap(),
        first
    );
}
