#[cfg(test)]
use rusqlite::OptionalExtension;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use super::{schema, Error};

#[cfg(test)]
type VersionObservation = Option<(
    Option<Vec<u8>>,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
)>;

const LINUX_O_NOFOLLOW: i32 = 0o400000;
#[cfg(test)]
use std::{cell::RefCell, rc::Rc};
#[cfg(test)]
thread_local! { static OPEN_REPLACEMENT_HOOK: RefCell<Option<fn(&Path)>> = const { RefCell::new(None) }; }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
    owner_uid: u32,
    mode: u32,
}

impl FileIdentity {
    fn from_metadata(meta: &std::fs::Metadata) -> Result<Self, Error> {
        validate_store_metadata(meta)?;
        Ok(Self {
            dev: meta.dev(),
            ino: meta.ino(),
            owner_uid: meta.uid(),
            mode: meta.mode(),
        })
    }
}

fn current_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(u32::MAX)
}
fn validate_store_metadata(meta: &std::fs::Metadata) -> Result<(), Error> {
    if !meta.is_file() || meta.uid() != current_uid() || meta.mode() & 0o7777 != 0o600 {
        Err(Error::AmbiguousSchema)
    } else {
        Ok(())
    }
}
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn current_uid_for_test() -> u32 {
    current_uid()
}
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn validate_store_metadata_for_test(
    meta: &std::fs::Metadata,
    expected_uid: u32,
) -> Result<(), Error> {
    if !meta.is_file() || meta.uid() != expected_uid || meta.mode() & 0o7777 != 0o600 {
        Err(Error::AmbiguousSchema)
    } else {
        Ok(())
    }
}
pub(super) fn validate_parent(path: &Path) -> Result<(), Error> {
    let mut parent = path.parent();
    while let Some(component) = parent {
        let meta = std::fs::symlink_metadata(component).map_err(|_| Error::AmbiguousSchema)?;
        if meta.file_type().is_symlink() {
            return Err(Error::AmbiguousSchema);
        }
        parent = component.parent();
    }
    Ok(())
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) struct OpenReplacementGuard {
    _not_send_sync: Rc<()>,
}
#[cfg(test)]
impl Drop for OpenReplacementGuard {
    fn drop(&mut self) {
        OPEN_REPLACEMENT_HOOK.with(|hook| *hook.borrow_mut() = None);
    }
}
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn replace_before_sqlite_open(hook: fn(&Path)) -> OpenReplacementGuard {
    OPEN_REPLACEMENT_HOOK.with(|slot| *slot.borrow_mut() = Some(hook));
    OpenReplacementGuard {
        _not_send_sync: Rc::new(()),
    }
}

pub(crate) struct Store {
    pub(super) connection: Connection,
}

#[allow(dead_code)]
impl Store {
    pub(crate) fn open_in_memory() -> Result<Self, Error> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        validate_parent(path)?;
        let meta = match std::fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match std::fs::OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .custom_flags(LINUX_O_NOFOLLOW)
                .open(path)
            {
                Ok(_) => std::fs::symlink_metadata(path).map_err(|_| Error::AmbiguousSchema)?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    std::fs::symlink_metadata(path).map_err(|_| Error::AmbiguousSchema)?
                }
                Err(_) => return Err(Error::AmbiguousSchema),
            },
            Err(_) => return Err(Error::AmbiguousSchema),
        };
        validate_store_metadata(&meta)?;
        let identity = FileIdentity::from_metadata(&meta)?;
        #[cfg(test)]
        OPEN_REPLACEMENT_HOOK.with(|hook| {
            if let Some(hook) = *hook.borrow() {
                hook(path);
            }
        });
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let opened = std::fs::symlink_metadata(path).map_err(|_| Error::AmbiguousSchema)?;
        if FileIdentity::from_metadata(&opened).ok() != Some(identity) {
            drop(connection);
            return Err(Error::AmbiguousSchema);
        }
        Self::from_connection(connection)
    }
    pub(crate) fn from_connection(mut connection: Connection) -> Result<Self, Error> {
        connection.execute_batch("PRAGMA foreign_keys=ON")?;
        let tx = connection.transaction()?;
        schema::initialize(&tx)?;
        tx.commit()?;
        Ok(Self { connection })
    }
    pub(super) fn schema_version(&self) -> Result<i64, Error> {
        Ok(self.connection.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?)
    }
    #[cfg(test)]
    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }
    #[cfg(test)]
    pub(crate) fn version_observation(&self, artifact: &str) -> Result<VersionObservation, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT logical_path,observed_identity,git_applicability,git_repository_path,git_head_state,git_head_oid,git_index_present,git_index_fingerprint FROM versions WHERE artifact_id=? ORDER BY rowid DESC LIMIT 1",
                [artifact],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .optional()?)
    }
    #[cfg(test)]
    pub(crate) fn accepted_record_count(&self, operation: &str) -> Result<i64, Error> {
        Ok(self.connection.query_row(
            "SELECT count(*) FROM operations WHERE id=? AND result='accepted'",
            [operation],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
pub(super) fn invoke_open_replacement_hook(path: &Path) {
    OPEN_REPLACEMENT_HOOK.with(|hook| {
        if let Some(hook) = *hook.borrow() {
            hook(path);
        }
    });
}

pub(crate) fn observe(path: &Path, expected: Option<&str>) -> Result<(String, String), Error> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(LINUX_O_NOFOLLOW)
        .open(path)
        .map_err(|_| Error::Drift)?;
    let before = f.metadata().map_err(|_| Error::Drift)?;
    if !before.is_file() {
        return Err(Error::Drift);
    }
    let identity = format!("{}:{}", before.dev(), before.ino());
    if expected.is_some_and(|expected| expected != identity) {
        return Err(Error::Drift);
    }
    let mut b = Vec::new();
    f.read_to_end(&mut b).map_err(|_| Error::Drift)?;
    let after = f.metadata().map_err(|_| Error::Drift)?;
    if before.dev() != after.dev() || before.ino() != after.ino() || before.len() != after.len() {
        return Err(Error::Drift);
    }
    Ok((identity, hash_bytes(&b)))
}
pub(crate) fn hash(s: &str) -> String {
    hash_bytes(s.as_bytes())
}
pub(crate) fn hash_bytes(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}
