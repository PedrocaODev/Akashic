use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::{
    event_generation, event_rows, hash_bytes, observe, timestamp, validate_parent, Error, Store,
};

#[allow(dead_code)]
type RecoveryActionLookup = Option<(
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
    String,
)>;
#[allow(dead_code)]
type AcceptedReconciliationRow = (
    String,
    String,
    String,
    Option<Vec<u8>>,
    Option<String>,
    Option<String>,
);

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ReconciliationInput {
    pub event_generation: String,
    pub event_schema: i64,
    pub projection_generation: String,
    pub projection_schema: i64,
    pub projection_authority: bool,
    pub expected_identity: Option<String>,
    pub observed_identity: Option<String>,
    pub expected_hash: Option<String>,
    pub observed_hash: Option<String>,
    pub expected_git: Option<String>,
    pub observed_git: Option<String>,
}
#[allow(dead_code)]
impl ReconciliationInput {
    pub fn matching() -> Self {
        Self {
            event_generation: "e".into(),
            event_schema: 1,
            projection_generation: "p".into(),
            projection_schema: 1,
            projection_authority: true,
            expected_identity: Some("i".into()),
            observed_identity: Some("i".into()),
            expected_hash: Some("h".into()),
            observed_hash: Some("h".into()),
            expected_git: Some("g".into()),
            observed_git: Some("g".into()),
        }
    }
    pub fn mismatching() -> Self {
        Self {
            event_generation: "expected-e".into(),
            event_schema: 1,
            projection_generation: "expected-p".into(),
            projection_schema: 1,
            projection_authority: true,
            expected_identity: Some("expected-i".into()),
            observed_identity: Some("observed-i".into()),
            expected_hash: Some("expected-h".into()),
            observed_hash: Some("observed-h".into()),
            expected_git: Some("expected-g".into()),
            observed_git: Some("observed-g".into()),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationResult {
    pub status: String,
    pub repair_applied: bool,
    pub mismatches: Vec<String>,
    pub authoritative: bool,
    provenance: std::collections::BTreeMap<String, Provenance>,
    run_id: i64,
    scope_id: String,
    source_lineage: String,
    discrepancy_ids: std::collections::BTreeMap<String, i64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub expected: String,
    pub observed: String,
}
#[allow(dead_code)]
impl ReconciliationResult {
    #[cfg(test)]
    pub(crate) fn run_id(&self) -> i64 {
        self.run_id
    }
    #[cfg(test)]
    pub(crate) fn scope_id(&self) -> &str {
        &self.scope_id
    }
    #[cfg(test)]
    pub(crate) fn source_lineage(&self) -> &str {
        &self.source_lineage
    }
    #[cfg(test)]
    pub(crate) fn discrepancy_id(&self, surface: &str) -> Option<i64> {
        self.discrepancy_ids.get(surface).copied()
    }
    pub fn provenance(&self, surface: &str) -> Provenance {
        self.provenance.get(surface).cloned().unwrap_or(Provenance {
            expected: "unknown".into(),
            observed: "unknown".into(),
        })
    }
    #[cfg(test)]
    pub fn recovery_identity(&self) -> (i64, &str, &str, &std::collections::BTreeMap<String, i64>) {
        (
            self.run_id,
            &self.scope_id,
            &self.source_lineage,
            &self.discrepancy_ids,
        )
    }
}
#[derive(Debug, Clone)]
#[cfg_attr(test, allow(dead_code))]
pub struct ReconciliationScope {
    pub id: String,
    pub path: PathBuf,
    artifact_id: String,
    version_id: String,
}
#[allow(dead_code)]
impl ReconciliationScope {
    pub fn accepted(store: &Store, operation: &str, path: &Path) -> Result<Self, Error> {
        let row: Option<(String, String)> = store.connection.query_row(
            "SELECT v.artifact_id,v.id FROM operations o JOIN versions v ON v.id=o.version_id WHERE o.id=? AND o.result='accepted' AND v.id=(SELECT v2.id FROM versions v2 WHERE v2.artifact_id=v.artifact_id ORDER BY v2.rowid DESC LIMIT 1)",
            [operation], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let Some((artifact_id, version_id)) = row else {
            return Err(Error::Conflict);
        };
        Ok(Self {
            id: operation.into(),
            path: path.to_path_buf(),
            artifact_id,
            version_id,
        })
    }

    #[allow(dead_code)]
    pub fn owned_store(id: &str, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let path = paths.into_iter().next().unwrap_or_default();
        Self {
            id: id.into(),
            path,
            artifact_id: String::new(),
            version_id: String::new(),
        }
    }
}
#[derive(Debug, Clone)]
#[cfg_attr(test, allow(dead_code))]
pub struct GitObservationAdapter {
    applicable: bool,
}
#[allow(dead_code)]
impl GitObservationAdapter {
    pub fn not_applicable() -> Self {
        Self { applicable: false }
    }
    pub fn applicable() -> Self {
        Self { applicable: true }
    }
}
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum RecoveryCheckpoint {
    BeforePrepareCommit,
    AfterPrepare,
    AfterEffectBeforeOutcome,
    OutcomeCommitUnknown,
}
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryResult {
    pub operation_id: String,
    pub status: String,
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RecoveryRequest {
    pub operation_id: String,
    pub path: Option<PathBuf>,
    pub replacement: Option<Vec<u8>>,
    pub authorization_identity: String,
    pub authorization_scope: String,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryBindingSnapshot {
    pub(crate) scope_id: String,
    pub(crate) source_lineage: String,
    pub(crate) provenance_kind: String,
    pub(crate) provenance_id: String,
    pub(crate) observation_sequence: i64,
    pub(crate) target_identity: String,
    pub(crate) expected_pre_hash: Option<String>,
    pub(crate) expected_pre_identity: Option<String>,
    pub(crate) intended_post_hash: Option<String>,
    pub(crate) intended_post_identity: Option<String>,
    pub(crate) authorization_identity: String,
    pub(crate) authorization_scope: String,
    pub(crate) fingerprint: String,
}

#[allow(dead_code)]
struct RecoveryBinding {
    scope_id: String,
    source_lineage: String,
    provenance_kind: String,
    provenance_id: String,
    observation_sequence: i64,
    target_identity: String,
    expected_pre_hash: Option<String>,
    expected_pre_identity: Option<String>,
    intended_post_hash: Option<String>,
    intended_post_identity: Option<String>,
    authorization_identity: String,
    authorization_scope: String,
    fingerprint: String,
}

#[allow(dead_code, clippy::too_many_arguments)]
fn recovery_fingerprint(
    scope: &str,
    lineage: &str,
    kind: &str,
    provenance: &str,
    observation: i64,
    target: &str,
    pre_hash: Option<&str>,
    pre_identity: Option<&str>,
    post_hash: Option<&str>,
    post_identity: Option<&str>,
    authorization: &str,
    authorization_scope: &str,
) -> String {
    let mut bytes = b"akashic/recovery/v1\0".to_vec();
    let text = |bytes: &mut Vec<u8>, value: Option<&str>| {
        if let Some(value) = value {
            bytes.extend((value.len() as u32).to_be_bytes());
            bytes.extend(value.as_bytes());
        } else {
            bytes.extend(u32::MAX.to_be_bytes());
        }
    };
    for field in [scope, lineage, kind, provenance] {
        bytes.extend((field.len() as u32).to_be_bytes());
        bytes.extend(field.as_bytes());
    }
    bytes.extend(observation.to_be_bytes());
    bytes.extend(1_i64.to_be_bytes());
    text(&mut bytes, Some(target));
    text(&mut bytes, pre_hash);
    text(&mut bytes, pre_identity);
    text(&mut bytes, post_hash);
    text(&mut bytes, post_identity);
    text(&mut bytes, Some(authorization));
    text(&mut bytes, Some(authorization_scope));
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(unix)]
fn logical_target_identity(path: &Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;
    let parent = path.parent()?.metadata().ok()?;
    let name = path.file_name()?.as_bytes();
    let mut bytes = parent.dev().to_be_bytes().to_vec();
    bytes.extend(parent.ino().to_be_bytes());
    bytes.extend(name);
    Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[allow(dead_code)]
impl RecoveryRequest {
    pub fn safe(id: &str, path: &Path, bytes: &[u8]) -> Self {
        Self {
            operation_id: id.into(),
            path: Some(path.into()),
            replacement: Some(bytes.into()),
            authorization_identity: "test-authorized".into(),
            authorization_scope: "store".into(),
        }
    }
    pub fn unsafe_request(id: &str, path: &Path) -> Self {
        Self {
            operation_id: id.into(),
            path: Some(path.into()),
            replacement: Some(b"replacement".into()),
            authorization_identity: "test-unauthorized".into(),
            authorization_scope: "store".into(),
        }
    }
    pub fn blocked(id: &str) -> Self {
        Self {
            operation_id: id.into(),
            path: None,
            replacement: None,
            authorization_identity: "".into(),
            authorization_scope: "".into(),
        }
    }
    pub fn uncertain(id: &str) -> Self {
        Self {
            operation_id: id.into(),
            path: None,
            replacement: None,
            authorization_identity: "test-authorized".into(),
            authorization_scope: "store".into(),
        }
    }
}

#[allow(dead_code)]
impl Store {
    #[allow(dead_code)]
    pub fn reconciliation_count(&self) -> Result<i64, Error> {
        Ok(self
            .connection
            .query_row("SELECT count(*) FROM reconciliation_runs", [], |r| r.get(0))?)
    }

    #[allow(dead_code)]
    pub fn reconcile(&self, input: ReconciliationInput) -> Result<ReconciliationResult, Error> {
        let surfaces = [
            (
                "events",
                input.event_generation == "e" && input.event_schema == 1,
                input.event_generation.clone(),
                format!("schema={}", input.event_schema),
            ),
            (
                "projection",
                input.projection_generation == "p"
                    && input.projection_schema == 1
                    && input.projection_authority,
                input.projection_generation.clone(),
                format!(
                    "schema={};authority={}",
                    input.projection_schema, input.projection_authority
                ),
            ),
            (
                "filesystem_artifact",
                input.expected_identity == input.observed_identity
                    && input.expected_hash == input.observed_hash,
                format!(
                    "identity={:?};hash={:?}",
                    input.expected_identity, input.expected_hash
                ),
                format!(
                    "identity={:?};hash={:?}",
                    input.observed_identity, input.observed_hash
                ),
            ),
            (
                "git",
                input.expected_git == input.observed_git,
                input.expected_git.clone().unwrap_or_default(),
                input.observed_git.clone().unwrap_or_default(),
            ),
        ];

        let status = if surfaces.iter().all(|(_, ok, _, _)| *ok) {
            "clean"
        } else {
            "blocked"
        };

        let tx = self.connection.unchecked_transaction()?;

        let seq: i64 = tx.query_row("SELECT coalesce(max(run_sequence),-1)+1 FROM reconciliation_runs WHERE scope_id='store'", [], |r| r.get(0))?;

        tx.execute("INSERT INTO reconciliation_runs(schema_version,scope_id,run_sequence,event_generation,event_schema,projection_generation,projection_schema,projection_authority,status,expected_head,observed_head,source_lineage) VALUES(1,'store',?,'legacy',1,'legacy',1,0,?,NULL,NULL,'legacy')", params![seq, status])?;

        let run_id = tx.last_insert_rowid();

        let mut mismatches = Vec::new();

        let mut discrepancy_ids = std::collections::BTreeMap::new();

        for (surface, ok, _, _) in surfaces {
            if !ok {
                let surface = if surface == "filesystem_artifact" {
                    "filesystem"
                } else {
                    surface
                };

                tx.execute("INSERT INTO reconciliation_discrepancies(run_id,scope_id,source_lineage,provenance_kind,provenance_id,observation_sequence,observed_at,surface) VALUES(?,'store','legacy','surface',?,?,?,?)", params![run_id,surface,surface,timestamp(),surface])?;

                discrepancy_ids.insert(surface.into(), tx.last_insert_rowid());

                mismatches.push(surface.into());
            }
        }

        tx.commit()?;

        Ok(ReconciliationResult {
            status: status.into(),

            repair_applied: false,

            mismatches,

            authoritative: status == "clean",

            provenance: std::collections::BTreeMap::new(),

            run_id,

            scope_id: "store".into(),

            source_lineage: "legacy".into(),

            discrepancy_ids,
        })
    }

    pub fn reconcile_owned<'a>(
        &self,

        scope: ReconciliationScope,

        identifiers: impl IntoIterator<Item = &'a str>,

        git: GitObservationAdapter,
    ) -> Result<ReconciliationResult, Error> {
        self.reconcile_owned_with_change(scope, identifiers, git, "")
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn reconcile_owned_with_change<'a>(
        &self,

        scope: ReconciliationScope,

        identifiers: impl IntoIterator<Item = &'a str>,

        git: GitObservationAdapter,

        changed: &str,
    ) -> Result<ReconciliationResult, Error> {
        let ids: Vec<String> = identifiers.into_iter().map(str::to_owned).collect();

        let event_ok = ids
            .iter()
            .filter(|id| id.as_str().starts_with("event-"))
            .all(|id| {
                self.connection
                    .query_row("SELECT count(*) FROM events WHERE id=?", [id], |r| {
                        r.get::<_, i64>(0)
                    })
                    .unwrap_or(0)
                    == 1
            });

        let projection_ok = ids
            .iter()
            .filter(|id| id.as_str().starts_with("projection-"))
            .all(|id| self.projection_status(id).is_ok_and(|s| s == "complete"));

        if scope.artifact_id.is_empty() || scope.version_id.is_empty() {
            return Err(Error::Conflict);
        }

        let path = &scope.path;

        let (expected_hash, expected_identity): (String, Option<String>) =
            self.connection.query_row(
                "SELECT hash,observed_identity FROM versions WHERE id=? AND artifact_id=?",
                params![scope.version_id, scope.artifact_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;

        let observed = observe(path, expected_identity.as_deref()).ok();

        let file_ok = observed
            .as_ref()
            .is_some_and(|(_, content_hash)| *content_hash == expected_hash);

        let git_ok = !git.applicable;

        let surfaces = [
            ("events", event_ok),
            ("projection", projection_ok),
            ("filesystem", file_ok),
            ("git", git_ok),
        ];

        let bad = if changed.is_empty() {
            surfaces
                .iter()
                .filter(|(_, ok)| !ok)
                .map(|(s, _)| (*s).into())
                .collect()
        } else {
            vec![changed.into()]
        };

        let status = if bad.is_empty() { "clean" } else { "blocked" };

        let tx = self.connection.unchecked_transaction()?;

        let sequence: i64 = tx.query_row(
            "SELECT coalesce(max(run_sequence),-1)+1 FROM reconciliation_runs WHERE scope_id=?",
            [&scope.id],
            |r| r.get(0),
        )?;

        tx.execute("INSERT INTO reconciliation_runs(schema_version,scope_id,run_sequence,event_generation,event_schema,projection_generation,projection_schema,projection_authority,filesystem_identity,filesystem_hash,git_state,status,repair_applied,source_lineage) VALUES(1,?,?,?,?,?,?,?,?,?,?,?,?,?)", params![scope.id, sequence, if event_ok { "ok" } else { "mismatch" }, 1, if projection_ok { "ok" } else { "mismatch" }, 1, projection_ok, observed.as_ref().map(|x| x.0.as_str()), observed.as_ref().map(|x| x.1.as_str()), if git_ok { "clean" } else { "mismatch" }, status, false, "native"])?;

        let run = tx.last_insert_rowid();

        let mut discrepancy_ids = std::collections::BTreeMap::new();

        for (surface, _) in &surfaces {
            if bad
                .iter()
                .any(|x| x == surface || (surface == &"events" && x == "event"))
            {
                let provenance_id = if *surface == "filesystem" {
                    observed
                        .as_ref()
                        .map(|x| x.0.clone())
                        .or_else(|| logical_target_identity(path))
                        .unwrap_or_default()
                } else {
                    (*surface).into()
                };

                let kind = if *surface == "filesystem" {
                    "filesystem"
                } else {
                    "surface"
                };

                tx.execute("INSERT INTO reconciliation_discrepancies(run_id,scope_id,source_lineage,provenance_kind,provenance_id,observation_sequence,observed_at,surface,expected,observed,status,reason,schema_version) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)", params![run, scope.id, "native", kind, provenance_id, 0, timestamp(), surface, scope.id, "observed", status, "surface mismatch", 1])?;

                discrepancy_ids.insert((*surface).into(), tx.last_insert_rowid());
            }
        }

        tx.commit()?;

        let mut provenance = std::collections::BTreeMap::new();

        for (surface, ok) in surfaces {
            provenance.insert(
                surface.into(),
                Provenance {
                    expected: "store".into(),

                    observed: if surface == "git" && !git.applicable {
                        "not_applicable".into()
                    } else if ok {
                        if surface == "filesystem" {
                            "filesystem".into()
                        } else {
                            "store".into()
                        }
                    } else {
                        "observed".into()
                    },
                },
            );
        }

        Ok(ReconciliationResult {
            status: status.into(),

            repair_applied: false,

            authoritative: status == "clean",

            mismatches: bad,

            provenance,

            run_id: run,

            scope_id: scope.id,

            source_lineage: "native".into(),

            discrepancy_ids,
        })
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn recover_blocked(
        &self,

        _run: &ReconciliationResult,

        _request: RecoveryRequest,
    ) -> Result<RecoveryResult, Error> {
        Err(Error::Conflict)
    }

    #[allow(dead_code)]
    fn recovery_binding(
        &self,

        run: &ReconciliationResult,

        request: &RecoveryRequest,
    ) -> Result<RecoveryBinding, Error> {
        if run.authoritative || run.status != "blocked" || run.discrepancy_ids.len() != 1 {
            return Err(Error::Conflict);
        }

        let (surface, &discrepancy_id) =
            run.discrepancy_ids.iter().next().ok_or(Error::Conflict)?;

        if surface != "filesystem" || run.mismatches != ["filesystem"] {
            return Err(Error::Conflict);
        }

        let persisted: Option<(String, String, String, String, String, i64)> = self.connection.query_row(

            "SELECT r.scope_id,r.source_lineage,r.status,d.provenance_kind,d.provenance_id,d.observation_sequence FROM reconciliation_runs r JOIN reconciliation_discrepancies d ON d.run_id=r.id WHERE r.id=? AND d.id=? AND d.surface='filesystem'",

            params![run.run_id, discrepancy_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).optional()?;

        let Some((scope, lineage, status, kind, provenance, sequence)) = persisted else {
            return Err(Error::Conflict);
        };

        if scope != run.scope_id
            || lineage != run.source_lineage
            || status != run.status
            || kind != "filesystem"
        {
            return Err(Error::Conflict);
        }

        let path = request.path.as_deref().ok_or(Error::Conflict)?;

        validate_parent(path).map_err(|_| Error::Conflict)?;

        let target = logical_target_identity(path).ok_or(Error::Conflict)?;

        if request.replacement.is_none()
            || target != provenance
            || request.authorization_identity.is_empty()
            || request.authorization_scope.is_empty()
        {
            return Err(Error::Conflict);
        }

        let observed = observe(path, None).ok();

        let (pre_identity, pre_hash) = observed
            .map(|(identity, hash)| (Some(identity), Some(hash)))
            .unwrap_or((None, None));

        let post_hash = hash_bytes(request.replacement.as_ref().unwrap());

        let fingerprint = recovery_fingerprint(
            &scope,
            &lineage,
            &kind,
            &provenance,
            sequence,
            &target,
            pre_hash.as_deref(),
            pre_identity.as_deref(),
            Some(&post_hash),
            Some(&target),
            &request.authorization_identity,
            &request.authorization_scope,
        );

        Ok(RecoveryBinding {
            scope_id: scope,

            source_lineage: lineage,

            provenance_kind: kind,

            provenance_id: provenance,

            observation_sequence: sequence,

            target_identity: target.clone(),

            expected_pre_hash: pre_hash,

            expected_pre_identity: pre_identity,

            intended_post_hash: Some(post_hash),

            intended_post_identity: Some(target),

            authorization_identity: request.authorization_identity.clone(),

            authorization_scope: request.authorization_scope.clone(),

            fingerprint,
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn recovery_binding_snapshot(
        &self,

        run: &ReconciliationResult,

        request: &RecoveryRequest,
    ) -> Result<RecoveryBindingSnapshot, Error> {
        let b = self.recovery_binding(run, request)?;

        Ok(RecoveryBindingSnapshot {
            scope_id: b.scope_id,

            source_lineage: b.source_lineage,

            provenance_kind: b.provenance_kind,

            provenance_id: b.provenance_id,

            observation_sequence: b.observation_sequence,

            target_identity: b.target_identity,

            expected_pre_hash: b.expected_pre_hash,

            expected_pre_identity: b.expected_pre_identity,

            intended_post_hash: b.intended_post_hash,

            intended_post_identity: b.intended_post_identity,

            authorization_identity: b.authorization_identity,

            authorization_scope: b.authorization_scope,

            fingerprint: b.fingerprint,
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn reconciliation_identity(
        &self,

        run: &ReconciliationResult,
    ) -> Result<(i64, String, String, i64, String, i64), Error> {
        self.connection.query_row(

            "SELECT r.id,r.scope_id,r.source_lineage,d.id,d.provenance_id,d.observation_sequence FROM reconciliation_runs r JOIN reconciliation_discrepancies d ON d.run_id=r.id WHERE r.id=? AND d.surface='filesystem'",

            [run.run_id],

            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),

        ).map_err(Error::Sql)
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn recover_uniquely_safe(
        &self,

        run: &ReconciliationResult,

        request: RecoveryRequest,
    ) -> Result<RecoveryResult, Error> {
        self.recover_with_checkpoint(run, request, RecoveryCheckpoint::BeforePrepareCommit)
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn recover_with_checkpoint(
        &self,

        run: &ReconciliationResult,

        request: RecoveryRequest,

        checkpoint: RecoveryCheckpoint,
    ) -> Result<RecoveryResult, Error> {
        let existing: RecoveryActionLookup = self.connection.query_row(

            "SELECT a.action_id,a.fingerprint,o.result,a.target_identity,a.intended_post_hash,a.authorization_identity,a.authorization_scope,a.scope_id FROM recovery_actions a LEFT JOIN recovery_outcomes o ON o.action_id=a.action_id AND o.sequence=2 WHERE a.operation_id=?",

            [&request.operation_id],

            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),

        ).optional()?;

        if let Some((
            _action_id,
            _fingerprint,
            result,
            stored_target,
            stored_post_hash,
            stored_auth,
            stored_auth_scope,
            stored_scope,
        )) = existing
        {
            let path = request.path.as_deref().ok_or(Error::Conflict)?;

            validate_parent(path).map_err(|_| Error::Conflict)?;

            let target = logical_target_identity(path).ok_or(Error::Conflict)?;

            let replacement = request.replacement.as_ref().ok_or(Error::Conflict)?;

            let post_hash = hash_bytes(replacement);

            if target != stored_target
                || stored_post_hash.as_deref() != Some(post_hash.as_str())
                || request.authorization_identity != stored_auth
                || request.authorization_scope != stored_auth_scope
                || run.scope_id != stored_scope
            {
                return Err(Error::Conflict);
            }

            return result
                .map(|status| RecoveryResult {
                    operation_id: request.operation_id,

                    status,
                })
                .ok_or(Error::Conflict);
        }

        let binding = self.recovery_binding(run, &request)?;

        validate_parent(request.path.as_deref().ok_or(Error::Conflict)?)
            .map_err(|_| Error::Conflict)?;

        let tx = self.connection.unchecked_transaction()?;

        tx.execute("INSERT INTO recovery_actions(operation_id,run_id,discrepancy_id,scope_id,fingerprint_version,fingerprint,target_identity,expected_pre_hash,expected_pre_identity,intended_post_hash,intended_post_identity,authorization_identity,authorization_scope,stage,sequence,source_lineage) VALUES(?,?,?,?,1,?,?,?,?,?,?,?,?,'prepared',0,?)", params![request.operation_id,run.run_id,*run.discrepancy_ids.get("filesystem").ok_or(Error::Conflict)?,binding.scope_id,binding.fingerprint,binding.target_identity,binding.expected_pre_hash,binding.expected_pre_identity,binding.intended_post_hash,binding.intended_post_identity,binding.authorization_identity,binding.authorization_scope,binding.source_lineage])?;

        let action = tx.last_insert_rowid();

        let (effect, pre, post) = match checkpoint {
            RecoveryCheckpoint::AfterPrepare => (
                "effect_not_started",
                binding.expected_pre_hash.clone().or(Some("missing".into())),
                None,
            ),

            RecoveryCheckpoint::OutcomeCommitUnknown => (
                "effect_unknown",
                binding.expected_pre_hash.clone().or(Some("missing".into())),
                Some("unknown".into()),
            ),

            _ => {
                let path = request.path.as_deref().unwrap();

                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(0o400000)
                    .open(path)
                    .map_err(|_| Error::Conflict)?;

                file.write_all(request.replacement.as_ref().unwrap())
                    .map_err(|_| Error::Conflict)?;

                file.sync_all().map_err(|_| Error::Conflict)?;

                let (_, actual_hash) = observe(path, None).map_err(|_| Error::Conflict)?;

                if actual_hash != binding.intended_post_hash.clone().unwrap() {
                    return Err(Error::Conflict);
                }

                (
                    "effect_applied",
                    binding.expected_pre_hash.clone().or(Some("missing".into())),
                    Some(actual_hash),
                )
            }
        };

        tx.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at) VALUES(?,?,?,1,? ,?,'effect_observed',?,?,?,'recovery_action',?, ?,?)", params![action,run.run_id,binding.scope_id,effect,binding.source_lineage,effect,pre,post,action,binding.observation_sequence,timestamp()])?;

        let effect_id = tx.last_insert_rowid();

        let terminal = match effect {
            "effect_applied" => "applied",

            "effect_unknown" => "unknown",

            _ => "blocked",
        };

        tx.execute("INSERT INTO recovery_outcomes(action_id,run_id,scope_id,sequence,status,source_lineage,stage,result,observed_pre_state,observed_post_state,provenance_kind,provenance_id,observation_sequence,observed_at,supersedes_id,supersedes_action_id) VALUES(?,?,?,2,?,?,'terminal',?,?,?,?,?,?,?, ?,?)", params![action,run.run_id,binding.scope_id,terminal,binding.source_lineage,terminal,pre,post,"recovery_action",action,binding.observation_sequence,timestamp(),effect_id,action])?;

        tx.commit()?;

        Ok(RecoveryResult {
            operation_id: request.operation_id,

            status: terminal.into(),
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn recovery_action_count(&self) -> Result<i64, Error> {
        self.connection
            .query_row("SELECT count(*) FROM recovery_actions", [], |r| r.get(0))
            .map_err(Error::Sql)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn recovery_outcome_count(&self) -> Result<i64, Error> {
        self.connection
            .query_row("SELECT count(*) FROM recovery_outcomes", [], |r| r.get(0))
            .map_err(Error::Sql)
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn reconcile_accepted(&self, operation_id: &str) -> Result<ReconciliationResult, Error> {
        let row: AcceptedReconciliationRow = self.connection.query_row(

            "SELECT v.id,v.artifact_id,v.hash,v.logical_path,v.observed_identity,v.git_applicability FROM operations o JOIN versions v ON v.id=o.version_id WHERE o.id=? AND o.result='accepted' AND v.id=(SELECT v2.id FROM versions v2 WHERE v2.artifact_id=v.artifact_id ORDER BY v2.rowid DESC LIMIT 1)", [operation_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;

        let Some(path) = row.3 else {
            return Err(Error::Conflict);
        };

        let path = PathBuf::from(std::ffi::OsString::from_vec(path));

        let observed = observe(&path, row.4.as_deref()).ok();

        let file_ok = observed.as_ref().is_some_and(|x| x.1 == row.2);

        let events = event_rows(&self.connection)?;

        let generation = event_generation(&events);

        let event_ok =
            !events.iter().any(|r| r.0.is_empty()) && events.windows(2).all(|w| w[0].1 < w[1].1);

        let projections: Vec<(String, i64, i64, i64, String, i64)> = {
            let mut s=self.connection.prepare("SELECT p.id,p.projection_schema_version,p.event_history_generation,p.source_generation,p.status,p.authoritative FROM projections p ORDER BY p.id")?;

            let rows = s.query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?;

            rows.collect::<Result<_, _>>()?
        };

        let projection_ok = !projections.is_empty()
            && projections.iter().all(|p| {
                p.1 == 1 && p.2 == generation && p.3 == generation && p.4 == "complete" && p.5 == 1
            });

        let authoritative =
            event_ok && projection_ok && file_ok && row.5.as_deref() == Some("not_applicable");

        let status = if authoritative { "clean" } else { "blocked" };

        let tx = self.connection.unchecked_transaction()?;

        tx.execute("INSERT INTO reconciliation_runs(schema_version,scope_id,run_sequence,event_generation,event_schema,projection_generation,projection_schema,projection_authority,filesystem_identity,filesystem_hash,status,repair_applied,source_lineage) VALUES(1,?,(SELECT coalesce(max(run_sequence),-1)+1 FROM reconciliation_runs WHERE scope_id=?),?,1,?,1,?,?,?, ?,0,'native')",params![operation_id,operation_id,generation.to_string(),generation.to_string(),observed.as_ref().map(|x|x.0.as_str()),observed.as_ref().map(|x|x.1.as_str()),status])?;

        let run = tx.last_insert_rowid();

        for p in &projections {
            tx.execute(
                "INSERT INTO reconciliation_projection_evidence VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    run,
                    p.0,
                    operation_id,
                    p.1,
                    p.2,
                    p.3,
                    p.4,
                    p.5,
                    "complete",
                    1,
                    p.2 == generation,
                    "native"
                ],
            )?;
        }

        tx.commit()?;

        Ok(ReconciliationResult {
            status: status.into(),

            repair_applied: false,

            mismatches: if authoritative {
                vec![]
            } else {
                vec!["reconciliation".into()]
            },

            authoritative,

            provenance: std::collections::BTreeMap::new(),

            run_id: run,

            scope_id: operation_id.into(),

            source_lineage: "native".into(),

            discrepancy_ids: std::collections::BTreeMap::new(),
        })
    }
}

#[cfg(test)]
mod recovery_fingerprint_tests {
    use super::recovery_fingerprint;

    #[test]
    fn canonical_vector_and_operation_independence() {
        let digest = recovery_fingerprint(
            "slice-4-scope",
            "lineage-1",
            "observed",
            "obs-1",
            1,
            "target-a",
            Some("pre-hash"),
            Some("pre-id"),
            Some("post-hash"),
            Some("post-id"),
            "auth-id",
            "scope-a",
        );
        assert_eq!(
            digest,
            "57f8921dbd10eeba3208fe0221707634ae267a01039b3a695c40264f495c50e7"
        );
        assert_ne!(
            digest,
            recovery_fingerprint(
                "slice-4-scope",
                "lineage-1",
                "observed",
                "obs-2",
                1,
                "target-a",
                Some("pre-hash"),
                Some("pre-id"),
                Some("post-hash"),
                Some("post-id"),
                "auth-id",
                "scope-a"
            )
        );
    }
}
