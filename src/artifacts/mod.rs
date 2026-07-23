mod lineage;
mod recovery;
mod replay;
mod schema;
mod store;
#[cfg(test)]
mod tests;
#[allow(unused_imports)]
pub use lineage::{Discrepancy, Import, Version};
pub(super) use replay::{event_generation, event_rows};
#[allow(unused_imports)]
pub use replay::{CapturedSimulation, Event, PlaybackResult, Projection};
use schema::timestamp;
pub(super) use schema::SCHEMA;
#[cfg(test)]
pub(crate) use schema::{
    canonical_descriptor, canonical_metadata, canonical_v1_connection, canonical_v2_connection,
    fail_next_migration_checkpoint, test_decision_fingerprint,
};
#[cfg(test)]
use store::invoke_open_replacement_hook;
use store::validate_parent;
#[cfg(test)]
pub(crate) use store::{
    current_uid_for_test, replace_before_sqlite_open, validate_store_metadata_for_test,
};
pub(super) use store::{hash, hash_bytes, observe, Store};

#[allow(dead_code)]
#[derive(Debug)]
pub enum Error {
    #[cfg_attr(test, allow(dead_code))]
    Sql(rusqlite::Error),
    UnsupportedSchema(i64),
    AmbiguousSchema,
    Conflict,
    OutOfOrder,
    Drift,
    #[allow(dead_code)]
    RebuildFailure,
    #[cfg_attr(test, allow(dead_code))]
    Discrepancy(rusqlite::Error),
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl Error {
    #[cfg_attr(test, allow(dead_code))]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema(_) => "config.unsupported_schema",
            Self::AmbiguousSchema => "config.ambiguous_schema",
            _ => "internal.unexpected",
        }
    }
    #[cfg_attr(test, allow(dead_code))]
    pub fn message(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema(_) => "unsupported artifact store schema",
            Self::AmbiguousSchema => "ambiguous artifact store schema",
            _ => "artifact store unavailable",
        }
    }
}

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use recovery::RecoveryBindingSnapshot;
#[allow(unused_imports)]
pub use recovery::{
    GitObservationAdapter, Provenance, ReconciliationInput, ReconciliationResult,
    ReconciliationScope, RecoveryCheckpoint, RecoveryRequest, RecoveryResult,
};
