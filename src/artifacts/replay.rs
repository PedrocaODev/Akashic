use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use super::{Error, Store, SCHEMA};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Event {
    pub id: String,
    pub sequence: i64,
    pub transition: String,
    pub input: String,
    pub outcome: String,
}
impl Event {
    #[allow(dead_code)]
    pub fn new(id: &str, sequence: i64, transition: &str, input: &str, outcome: &str) -> Self {
        Self {
            id: id.into(),
            sequence,
            transition: transition.into(),
            input: input.into(),
            outcome: outcome.into(),
        }
    }
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Projection {
    pub id: String,
    pub projection_schema_version: i64,
    pub event_history_generation: i64,
    pub source_generation: i64,
    pub authoritative: bool,
    pub status: String,
    pub payload: String,
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PlaybackResult {
    pub result: String,
    pub divergence: Option<String>,
    pub exact: bool,
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CapturedSimulation {
    pub label: String,
    pub outcome: String,
    pub exact_playback_evidence: bool,
}

impl Store {
    #[allow(dead_code)]
    pub fn append_event(&self, event: Event) -> Result<(), Error> {
        let maximum: Option<i64> = self.connection.query_row(
            "SELECT max(event_sequence) FROM events WHERE schema_version=1",
            [],
            |r| r.get(0),
        )?;
        if maximum.is_some_and(|maximum| event.sequence <= maximum) {
            return Err(Error::OutOfOrder);
        }
        self.connection.execute("INSERT INTO events(id,schema_version,source_lineage,event_sequence,transition,deterministic_input,outcome) VALUES(?,1,'native',?,?,?,?)", params![event.id, event.sequence, event.transition, event.input, event.outcome])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn rebuild_projection(&self, id: &str, schema: i64) -> Result<Projection, Error> {
        let tx = self.connection.unchecked_transaction()?;
        let mut events = tx.prepare("SELECT id,event_sequence,transition,deterministic_input,outcome FROM events WHERE schema_version=1 ORDER BY event_sequence,id")?;
        let rows = events
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(events);
        let generation = event_generation(&rows);
        let derived = (schema == SCHEMA).then(|| {
            rows.iter()
                .map(|r| derive(&r.2, &r.3))
                .collect::<Option<Vec<_>>>()
        });
        let (status, failure, payload) = match derived {
            Some(Some(values)) => ("complete", None, values.join("\n")),
            Some(None) => ("failed", Some("unknown transition"), String::new()),
            None => ("failed", Some("incompatible schema"), String::new()),
        };
        tx.execute("INSERT INTO projection_rebuilds(projection_id,event_id,event_sequence,status,failure,schema_version,source_lineage) VALUES(?,?,?,?,?,1,'native')", params![id, rows.last().map(|r| r.0.as_str()).unwrap_or(""), rows.last().map(|r| r.1).unwrap_or(0), status, failure])?;
        tx.execute("INSERT INTO projection_rebuild_status(projection_id,status,authoritative,failure,drift,schema_version,source_lineage) VALUES(?,?,?,?,NULL,1,'native') ON CONFLICT(projection_id) DO UPDATE SET status=excluded.status,authoritative=excluded.authoritative,failure=excluded.failure,drift=NULL,schema_version=1,source_lineage='native'", params![id,status, (status == "complete") as i64, failure]).map_err(Error::Sql)?;
        if status == "complete" {
            tx.execute("INSERT INTO projections(id,projection_schema_version,event_history_generation,source_generation,authoritative,status,payload,schema_version,source_lineage) VALUES(?,?,?,?,1,?,?,1,'native') ON CONFLICT(id) DO UPDATE SET projection_schema_version=excluded.projection_schema_version,event_history_generation=excluded.event_history_generation,source_generation=excluded.source_generation,authoritative=1,status=excluded.status,payload=excluded.payload,schema_version=1,source_lineage='native'", params![id, SCHEMA, generation, generation, status, payload])?;
        } else {
            tx.execute(
                "INSERT INTO projections(id,projection_schema_version,event_history_generation,source_generation,authoritative,status,payload,schema_version,source_lineage) VALUES(?,?,?, ?,0,?,'',1,'native') ON CONFLICT(id) DO UPDATE SET authoritative=0,status=excluded.status,schema_version=1,source_lineage='native'",
                params![id, SCHEMA, generation, generation, status],
            )
            .map_err(Error::Sql)?;
        }
        tx.commit()?;
        if status != "complete" {
            return Err(Error::RebuildFailure);
        }
        Ok(self.projection(id)?.unwrap_or(Projection {
            id: id.into(),
            projection_schema_version: SCHEMA,
            event_history_generation: generation,
            source_generation: generation,
            authoritative: false,
            status: status.into(),
            payload,
        }))
    }

    #[allow(dead_code)]
    pub fn projection_status(&self, id: &str) -> Result<String, Error> {
        let Some(p) = self.projection(id)? else {
            self.mark_projection_status(id, "missing")?;
            return Ok("missing".into());
        };
        if p.status == "failed" {
            if p.authoritative {
                self.mark_projection_status(id, "failed")?;
            }
            return Ok("failed".into());
        }
        let rows = event_rows(&self.connection)?;
        let generation = event_generation(&rows);
        if p.projection_schema_version != SCHEMA || p.event_history_generation != generation {
            self.mark_projection_status(id, "stale")?;
            return Ok("stale".into());
        }
        let expected = self.connection.prepare("SELECT transition,deterministic_input FROM events WHERE schema_version=1 ORDER BY event_sequence,id")?.query_map([], |r| { let t: String = r.get(0)?; let i: String = r.get(1)?; Ok(derive(&t, &i)) })?.collect::<Result<Option<Vec<_>>,_>>()?;
        let status = if expected.is_some_and(|v| p.payload == v.join("\n") && p.authoritative) {
            "complete"
        } else {
            "drifted"
        };
        if status != "complete" {
            self.mark_projection_status(id, status)?;
        }
        Ok(status.into())
    }

    fn mark_projection_status(&self, id: &str, status: &str) -> Result<(), Error> {
        self.connection.execute(
            "UPDATE projections SET authoritative=0,status=? WHERE id=?",
            params![status, id],
        )?;
        self.connection.execute("INSERT INTO projection_rebuild_status(projection_id,status,authoritative,failure,drift,schema_version,source_lineage) VALUES(?,?,0,NULL,?,1,'native') ON CONFLICT(projection_id) DO UPDATE SET status=excluded.status,authoritative=0,drift=excluded.drift,schema_version=1,source_lineage='native'", params![id,status,status])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn projection(&self, id: &str) -> Result<Option<Projection>, Error> {
        let Some(mut projection) = self.connection.query_row("SELECT id,projection_schema_version,event_history_generation,source_generation,authoritative,status,payload FROM projections WHERE id=?", [id], |r| Ok(Projection { id:r.get(0)?, projection_schema_version:r.get(1)?, event_history_generation:r.get(2)?, source_generation:r.get(3)?, authoritative:r.get::<_,i64>(4)? != 0, status:r.get(5)?, payload:r.get(6)? })).optional()? else { return Ok(None) };
        let rows = event_rows(&self.connection)?;
        let generation = event_generation(&rows);
        let expected = rows
            .iter()
            .map(|r| derive(&r.2, &r.3))
            .collect::<Option<Vec<_>>>();
        let status = if projection.status == "failed"
            || projection.projection_schema_version != SCHEMA
            || projection.event_history_generation != generation
            || projection.source_generation != generation
        {
            if projection.status == "failed" {
                "failed"
            } else {
                "stale"
            }
        } else if expected
            .as_ref()
            .is_none_or(|values| projection.payload != values.join("\n"))
        {
            "drifted"
        } else {
            "complete"
        };
        if status != "complete" || !projection.authoritative {
            self.mark_projection_status(id, status)?;
            projection.authoritative = false;
            projection.status = status.into();
        }
        Ok(Some(projection))
    }

    #[allow(dead_code)]
    pub fn playback(
        &self,
        id: &str,
        generation: i64,
        transitions: &[&str],
        outcomes: &[&str],
    ) -> Result<PlaybackResult, Error> {
        let rows = event_rows(&self.connection)?;
        let length = (generation >= 0)
            .then(|| {
                (0..=rows.len()).find(|&length| event_generation(&rows[..length]) == generation)
            })
            .flatten()
            .unwrap_or(0);
        let inputs = rows[..length]
            .iter()
            .map(|row| row.3.clone())
            .collect::<Vec<_>>();
        let input_refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
        self.playback_with_inputs(id, generation, transitions, &input_refs, outcomes)
    }

    pub fn playback_with_inputs(
        &self,
        id: &str,
        generation: i64,
        transitions: &[&str],
        inputs: &[&str],
        outcomes: &[&str],
    ) -> Result<PlaybackResult, Error> {
        let rows = event_rows(&self.connection)?;
        let selected = if generation >= 0 {
            (0..=rows.len()).find(|&length| event_generation(&rows[..length]) == generation)
        } else {
            None
        };
        let persisted: Vec<(String, String, Option<String>)> = rows
            .iter()
            .take(selected.unwrap_or(0))
            .map(|r| (r.2.clone(), r.3.clone(), r.4.clone()))
            .collect();
        let exact = generation >= 0
            && selected.is_some()
            && persisted.len() == transitions.len()
            && persisted.len() == inputs.len()
            && persisted.len() == outcomes.len()
            && transitions.len() == inputs.len()
            && inputs.len() == outcomes.len()
            && persisted
                .iter()
                .zip(transitions)
                .zip(inputs)
                .zip(outcomes)
                .all(|(((p, t), i), o)| {
                    p.0 == *t
                        && p.1 == *i
                        && p.2.as_deref() == Some(*o)
                        && derive(t, i).is_some_and(|v| v == *o)
                });
        let result = if exact {
            "exact_playback_succeeded"
        } else {
            "diverged"
        };
        let divergence = if exact {
            None
        } else if persisted
            .iter()
            .zip(outcomes)
            .any(|(p, o)| p.2.as_deref() != Some(*o))
        {
            Some("persisted outcome mismatch")
        } else {
            Some("transition or result mismatch")
        };
        self.connection.execute("INSERT INTO playback_results(id,event_history_generation,result,divergence,exact,schema_version,source_lineage) VALUES(?,?,?,?,?,1,'native')", params![id,generation,result,divergence,exact as i64])?;
        Ok(PlaybackResult {
            result: result.into(),
            divergence: divergence.map(str::to_owned),
            exact,
        })
    }

    #[allow(dead_code)]
    pub fn simulate_captured_outcome(
        &self,
        id: &str,
        outcome: &str,
    ) -> Result<CapturedSimulation, Error> {
        self.connection.execute("INSERT INTO captured_outcome_simulations(id,label,outcome,exact_playback_evidence,schema_version,source_lineage) VALUES(?, 'captured_outcome_simulation', ?,0,1,'native')", params![id,outcome])?;
        Ok(CapturedSimulation {
            label: "captured_outcome_simulation".into(),
            outcome: outcome.into(),
            exact_playback_evidence: false,
        })
    }
    #[allow(dead_code)]
    pub fn exact_playback_evidence(&self, id: &str) -> Result<(), Error> {
        let generation: Option<i64> = self.connection.query_row(
            "SELECT event_history_generation FROM playback_results WHERE id=? AND result='exact_playback_succeeded' AND exact=1 AND divergence IS NULL AND schema_version=1 AND source_lineage != ''",
            [id],
            |r| r.get(0),
        ).optional()?;
        let Some(generation) = generation else {
            return Err(Error::Conflict);
        };
        let rows = event_rows(&self.connection)?;
        if (0..=rows.len()).any(|length| event_generation(&rows[..length]) == generation) {
            Ok(())
        } else {
            Err(Error::Conflict)
        }
    }
}

fn derive(transition: &str, input: &str) -> Option<String> {
    match transition {
        "uppercase" | "t1" | "t2" => Some(input.to_uppercase()),
        "identity" | "set" => Some(input.to_owned()),
        _ => None,
    }
}

type EventRow = (String, i64, String, String, Option<String>);

pub(crate) fn event_rows(db: &Connection) -> Result<Vec<EventRow>, Error> {
    Ok(db.prepare("SELECT id,event_sequence,transition,deterministic_input,outcome FROM events WHERE schema_version=1 ORDER BY event_sequence,id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?)
}

pub(crate) fn event_generation(rows: &[EventRow]) -> i64 {
    let mut hash = Sha256::new();
    for row in rows {
        hash_field(&mut hash, row.0.as_bytes());
        hash_field(&mut hash, row.1.to_string().as_bytes());
        hash_field(&mut hash, row.2.as_bytes());
        hash_field(&mut hash, row.3.as_bytes());
        match row.4.as_deref() {
            Some(value) => {
                hash.update([1]);
                hash_field(&mut hash, value.as_bytes());
            }
            None => hash.update([0]),
        }
    }
    i64::from_be_bytes(hash.finalize()[..8].try_into().unwrap()) & i64::MAX
}

fn hash_field(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}
