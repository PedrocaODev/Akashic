use rusqlite::{params, OptionalExtension};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[cfg(test)]
use super::invoke_open_replacement_hook;
use super::{hash, observe, Error, Store, SCHEMA};

#[allow(dead_code)]
fn fingerprint(r: &Import<'_>, logical_path: Option<&[u8]>) -> String {
    let base = fingerprint_parts(
        r.actor,
        r.source,
        Some(r.artifact.unwrap_or(r.source)),
        &hash(r.content),
        None,
        r.ancestry,
    );
    hash(&format!("{}\0logical_path={:?}", base, logical_path))
}
#[allow(dead_code)]
fn normalize_logical_path(path: &Path) -> Result<Vec<u8>, Error> {
    let parent = path
        .parent()
        .ok_or(Error::Conflict)?
        .canonicalize()
        .map_err(|_| Error::Conflict)?;
    let name = path.file_name().ok_or(Error::Conflict)?;
    let mut bytes = parent.into_os_string().into_vec();
    bytes.push(b'/');
    bytes.extend_from_slice(std::os::unix::ffi::OsStrExt::as_bytes(name));
    Ok(bytes)
}
pub(super) fn fingerprint_parts(
    actor: &str,
    source: &str,
    artifact: Option<&str>,
    content: &str,
    _expected: Option<&str>,
    ancestry: Option<&str>,
) -> String {
    hash(&format!(
        "actor={}\0source={}\0artifact={:?}\0content={}\0ancestry={:?}",
        actor, source, artifact, content, ancestry
    ))
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct Import<'a> {
    operation: &'a str,
    actor: &'a str,
    source: &'a str,
    content: &'a str,
    file: Option<PathBuf>,
    expected_hash: Option<&'a str>,
    ancestry: Option<&'a str>,
    artifact: Option<&'a str>,
    expected_identity: Option<String>,
}
#[allow(dead_code)]
impl<'a> Import<'a> {
    pub fn new(operation: &'a str, actor: &'a str, source: &'a str, content: &'a str) -> Self {
        Self {
            operation,
            actor,
            source,
            content,
            file: None,
            expected_hash: None,
            ancestry: None,
            artifact: None,
            expected_identity: None,
        }
    }
    pub fn file(mut self, path: &Path) -> Self {
        self.file = Some(path.to_path_buf());
        self.expected_identity = std::fs::symlink_metadata(path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| format!("{}:{}", m.dev(), m.ino()));
        self
    }
    pub fn artifact(mut self, id: &'a str) -> Self {
        self.artifact = Some(id);
        self
    }
    pub fn expected_hash(mut self, hash: &'a str) -> Self {
        self.expected_hash = Some(hash);
        self
    }
    pub fn ancestry(mut self, ancestry: Option<&'a str>) -> Self {
        self.ancestry = ancestry;
        self
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Version {
    pub artifact_id: String,
    pub version_id: String,
    pub hash: String,
    pub canonical: String,
    pub ancestry: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Discrepancy {
    pub id: i64,
    pub expected_hash: Option<String>,
    pub observed_hash: Option<String>,
    pub expected_identity: Option<String>,
    pub observed_identity: Option<String>,
    pub proposed_hash: Option<String>,
    pub reason: String,
    pub status: String,
    pub result: String,
    pub schema_version: i64,
    pub operation: String,
    pub actor: String,
    pub artifact_id: String,
    pub source: String,
    pub ancestry: Option<String>,
    pub observed_owner: Option<String>,
    pub observed_source: Option<String>,
    pub observed_ancestry: Option<String>,
}

#[allow(dead_code)]
impl Store {
    pub fn version_count(&self) -> Result<i64, Error> {
        Ok(self
            .connection
            .query_row("SELECT count(*) FROM versions", [], |r| r.get(0))?)
    }

    pub fn discrepancy_count(&self) -> Result<i64, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM discrepancies) + (SELECT count(*) FROM reconciliation_discrepancies)",
                [],
                |r| r.get(0),
            )?)
    }

    pub fn discrepancy_context(&self, operation: &str) -> Result<Option<String>, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT context FROM discrepancies WHERE operation=?",
                [operation],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn discrepancy(&self, operation: &str) -> Result<Option<Discrepancy>, Error> {
        Ok(self.connection.query_row("SELECT id,expected_hash,observed_hash,expected_identity,observed_identity,proposed_hash,reason,status,operation_result_id,schema_version,operation,actor,artifact_id,source,ancestry,observed_owner,observed_source,observed_ancestry FROM discrepancies WHERE operation=?", [operation], |r| Ok(Discrepancy { id:r.get(0)?, expected_hash:r.get(1)?, observed_hash:r.get(2)?, expected_identity:r.get(3)?, observed_identity:r.get(4)?, proposed_hash:r.get(5)?, reason:r.get(6)?, status:r.get(7)?, result:r.get(8)?, schema_version:r.get(9)?, operation:r.get(10)?, actor:r.get(11)?, artifact_id:r.get(12)?, source:r.get(13)?, ancestry:r.get(14)?, observed_owner:r.get(15)?, observed_source:r.get(16)?, observed_ancestry:r.get(17)? })).optional()?)
    }

    pub fn operation_lineage(&self, operation: &str) -> Result<Option<Option<String>>, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT lineage FROM operations WHERE id=?",
                [operation],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn import(&self, r: Import<'_>) -> Result<Version, Error> {
        let normalized_path = r.file.as_deref().map(normalize_logical_path).transpose()?;
        let normalized_os_path = normalized_path
            .as_ref()
            .map(|p| PathBuf::from(std::ffi::OsString::from_vec(p.clone())));
        let fp = fingerprint(&r, normalized_path.as_deref());
        if r.file.is_some() && r.artifact.is_none() {
            return self.reject(
                &r,
                "implicit artifact discovery is forbidden",
                "conflict",
                Error::Conflict,
                &fp,
            );
        }
        if let Some((old, result, version, code)) = self
            .connection
            .query_row(
                "SELECT fingerprint,result,version_id,rejection_code FROM operations WHERE id=?",
                [r.operation],
                |q| {
                    Ok((
                        q.get::<_, String>(0)?,
                        q.get::<_, String>(1)?,
                        q.get::<_, Option<String>>(2)?,
                        q.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
        {
            let legacy_match = r.file.is_none()
                && old
                    == fingerprint_parts(
                        r.actor,
                        r.source,
                        Some(r.artifact.unwrap_or(r.source)),
                        &hash(r.content),
                        None,
                        r.ancestry,
                    );
            if old != fp && !legacy_match {
                return self.reject(
                    &r,
                    "operation fingerprint conflict",
                    "conflict",
                    Error::Conflict,
                    &fp,
                );
            }
            if result == "rejected" {
                return Err(match code.as_deref() {
                    Some("drift") => Error::Drift,
                    _ => Error::Conflict,
                });
            }
            return self.version(version.unwrap());
        }
        let artifact = r.artifact.unwrap_or(r.source).to_owned();
        let existing: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT owner,source FROM artifacts WHERE id=?",
                [&artifact],
                |q| Ok((q.get(0)?, q.get(1)?)),
            )
            .optional()?;
        if existing
            .as_ref()
            .is_some_and(|x| x != &(r.actor.to_owned(), r.source.to_owned()))
        {
            return self.reject(
                &r,
                "artifact owner/source conflict",
                "conflict",
                Error::Conflict,
                &fp,
            );
        }
        let latest: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM versions WHERE artifact_id=? ORDER BY rowid DESC LIMIT 1",
                [&artifact],
                |q| q.get(0),
            )
            .optional()?;
        let bound_path: Option<Vec<u8>> = self.connection.query_row(
            "SELECT logical_path FROM versions WHERE artifact_id=? AND logical_path IS NOT NULL ORDER BY rowid LIMIT 1",
            [&artifact], |q| q.get(0)).optional()?;
        if r.file.is_none() && bound_path.is_some() {
            return self.reject(
                &r,
                "file binding conflict",
                "conflict",
                Error::Conflict,
                &fp,
            );
        }
        if let (Some(path), Some(version_id)) = (normalized_os_path.as_deref(), latest.as_deref()) {
            let durable: (Option<Vec<u8>>, Option<String>, String) = self.connection.query_row(
                "SELECT logical_path,observed_identity,hash FROM versions WHERE id=?",
                [version_id],
                |q| Ok((q.get(0)?, q.get(1)?, q.get(2)?)),
            )?;
            if let Some(bound) = bound_path.or(durable.0) {
                if bound != normalized_path.as_ref().unwrap().as_slice() {
                    return self.reject(
                        &r,
                        "logical path conflict",
                        "conflict",
                        Error::Conflict,
                        &fp,
                    );
                }
            }
            let current = observe(path, durable.1.as_deref()).ok();
            if current.as_ref().map(|x| &x.0) != durable.1.as_ref()
                || current.as_ref().map(|x| &x.1) != Some(&durable.2)
            {
                let mut drift = r.clone();
                drift.expected_hash = Some(&durable.2);
                drift.expected_identity = durable.1.clone();
                return self.reject(
                    &drift,
                    "durable file identity drift",
                    "drift",
                    Error::Drift,
                    &fp,
                );
            }
        }
        if latest != r.ancestry.map(str::to_owned) {
            return self.reject(&r, "lineage conflict", "conflict", Error::Conflict, &fp);
        }
        if let Some(parent) = r.ancestry {
            let ok: i64 = self.connection.query_row("SELECT count(*) FROM versions WHERE id=? AND artifact_id=? AND actor=? AND source=?", params![parent,artifact,r.actor,r.source], |q| q.get(0))?;
            if ok == 0 {
                return self.reject(
                    &r,
                    "invalid version ancestry",
                    "conflict",
                    Error::Conflict,
                    &fp,
                );
            }
        }
        let content_hash = hash(r.content);
        if r.expected_hash.is_some_and(|h| h != content_hash) {
            return self.reject(&r, "expected hash mismatch", "drift", Error::Drift, &fp);
        }
        let observed = match normalized_os_path
            .as_deref()
            .map(|p| observe(p, r.expected_identity.as_deref()))
            .transpose()
        {
            Ok(value) => value,
            Err(error) => return self.reject(&r, "file observation failed", "drift", error, &fp),
        };
        if observed.as_ref().is_some_and(|(_, h)| h != &content_hash) {
            return self.reject(&r, "file content drift", "drift", Error::Drift, &fp);
        }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO artifacts(id,owner,source) VALUES(?,?,?)",
            params![artifact, r.actor, r.source],
        )?;
        let id = format!(
            "v-{}",
            hash(&format!(
                "artifact={artifact}\0hash={content_hash}\0operation={}\0actor={}\0source={}",
                r.operation, r.actor, r.source
            ))
        );
        tx.execute("INSERT INTO versions(id,artifact_id,hash,canonical,ancestry,operation,actor,source,observed_identity,expected_hash,source_schema_version,source_lineage,schema_version,logical_path,git_applicability) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",params![id,artifact,content_hash,r.content,r.ancestry,r.operation,r.actor,r.source,observed.as_ref().map(|x|&x.0),r.expected_hash,SCHEMA,"native",SCHEMA,normalized_path,normalized_path.as_ref().map(|_| "not_applicable")])?;
        if let Some(path) = normalized_os_path.as_deref() {
            #[cfg(test)]
            invoke_open_replacement_hook(path);
            let final_observed =
                observe(path, observed.as_ref().map(|x| x.0.as_str())).map_err(|_| Error::Drift)?;
            if final_observed != observed.clone().unwrap() {
                return Err(Error::Drift);
            }
        }
        tx.execute("INSERT INTO operations(id,fingerprint,result,version_id,lineage,rejection_code,schema_version) VALUES(?,?, 'accepted',?,?,NULL,?)",params![r.operation,fp,id,r.ancestry,SCHEMA])?;
        tx.execute(
            "INSERT INTO lineage(operation_id,version_id,parent_version_id,schema_version) VALUES(?,?,?,?)",
            params![r.operation, id, r.ancestry, SCHEMA],
        )?;
        tx.commit()?;
        self.version(id)
    }

    fn version(&self, id: String) -> Result<Version, Error> {
        Ok(self.connection.query_row(
            "SELECT artifact_id,hash,canonical,ancestry FROM versions WHERE id=?",
            [id.clone()],
            |q| {
                Ok(Version {
                    artifact_id: q.get(0)?,
                    version_id: id,
                    hash: q.get(1)?,
                    canonical: q.get(2)?,
                    ancestry: q.get(3)?,
                })
            },
        )?)
    }

    fn reject(
        &self,
        r: &Import<'_>,
        reason: &str,
        code: &str,
        error: Error,
        fp: &str,
    ) -> Result<Version, Error> {
        let (observed_identity, observed_hash) = r
            .file
            .as_deref()
            .and_then(|path| observe(path, None).ok())
            .map_or((None, None), |(identity, hash)| {
                (Some(identity), Some(hash))
            });
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(Error::Discrepancy)?;
        let accepted: Option<String> = tx
            .query_row(
                "SELECT version_id FROM operations WHERE id=? AND result='accepted'",
                [r.operation],
                |q| q.get(0),
            )
            .optional()
            .map_err(Error::Discrepancy)?;
        if accepted.is_none() {
            tx.execute("INSERT OR IGNORE INTO operations(id,fingerprint,result,version_id,lineage,rejection_code,schema_version) VALUES(?,?, 'rejected',NULL,NULL,?,?)",params![r.operation,fp,code,SCHEMA]).map_err(Error::Discrepancy)?;
        }
        let proposed_hash = Some(hash(r.content));
        let artifact = r.artifact.unwrap_or(r.source);
        let observed_owner: Option<String> = tx
            .query_row("SELECT owner FROM artifacts WHERE id=?", [artifact], |q| {
                q.get(0)
            })
            .optional()
            .map_err(Error::Discrepancy)?;
        let observed_source: Option<String> = tx
            .query_row("SELECT source FROM artifacts WHERE id=?", [artifact], |q| {
                q.get(0)
            })
            .optional()
            .map_err(Error::Discrepancy)?;
        let observed_ancestry: Option<String> = tx
            .query_row(
                "SELECT id FROM versions WHERE artifact_id=? ORDER BY rowid DESC LIMIT 1",
                [artifact],
                |q| q.get(0),
            )
            .optional()
            .map_err(Error::Discrepancy)?;
        let operation_result_id = accepted.as_deref().unwrap_or(r.operation);
        tx.execute("INSERT OR IGNORE INTO discrepancies(operation,actor,artifact_id,source,ancestry,reason,status,operation_result_id,expected_hash,observed_hash,expected_identity,observed_identity,proposed_hash,context,schema_version,observed_owner,observed_source,observed_ancestry) VALUES(?,?,?,?,?,?,'rejected',?,?,?,?,?,?,?, ?,?,?,?)",params![r.operation,r.actor,artifact,r.source,r.ancestry,reason,operation_result_id, r.expected_hash,observed_hash,r.expected_identity,observed_identity,proposed_hash,format!("schema={SCHEMA};code={code};original={}", accepted.as_deref().unwrap_or("")),SCHEMA,observed_owner,observed_source,observed_ancestry]).map_err(Error::Discrepancy)?;
        tx.commit().map_err(Error::Discrepancy)?;
        Err(error)
    }
}
