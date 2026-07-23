# event-replay-recovery Specification

## Purpose
TBD - created by archiving change build-artifact-runtime. Update Purpose after archive.
## Requirements
### Requirement: Append-only durable runtime history
SQLite MUST store events, findings, evidence, history, and replay metadata append-only. No operation MAY update or delete a durable record to repair, rewrite, or hide prior history; derived projections MUST NOT become an alternate source of truth.

#### Scenario: Append-only enforcement
- **WHEN** a repair or normal operation would modify or delete an existing event, finding, evidence, history, or replay record
- **THEN** the daemon MUST reject that mutation and MUST preserve the original record, using a new append-only record for any correction or discrepancy

### Requirement: Versioned schemas and reviewable migration
Every persisted event and runtime record format MUST carry an explicit schema version. Migrations MUST be explicit, reviewable, ordered, and preserve source lineage. An unsupported, ambiguous, or incompatible version MUST fail closed; the daemon MUST NOT guess, silently reinterpret, or destructively rewrite historical events.

#### Scenario: Migration
- **WHEN** a persisted record uses a supported older schema with an available reviewed migration
- **THEN** the daemon MUST apply the defined migration while preserving source lineage and record the compatibility decision

#### Scenario: Unsupported version
- **WHEN** startup, migration, or replay encounters an unsupported or ambiguous schema version
- **THEN** the operation MUST fail closed and MUST expose the version problem rather than treating the data as authoritative

### Requirement: Deterministic projections and visible drift
Projections MUST be versioned and identify the event-history generation and schema that produced them. A projection MUST be rebuildable in defined order from compatible events and deterministic inputs. The daemon MUST publish rebuild failure, incompleteness, or drift as visible non-authoritative status and MUST replace a projection only after successful reconstruction.

#### Scenario: Projection rebuild and drift
- **WHEN** a projection is missing, stale, or detected to differ from a rebuild from compatible events
- **THEN** the daemon MUST rebuild it deterministically when possible, record its resulting generation, and otherwise expose the failure or drift without silently treating the old projection as authoritative

### Requirement: Exact playback distinct from captured-outcome simulation
Exact event playback MUST re-apply the recorded event sequence and deterministic inputs through version-compatible deterministic transitions, MUST report any divergence, and MUST distinguish its result from captured-outcome simulation. Simulation MAY reuse recorded outcomes for analysis, but MUST be labeled as simulation and MUST NOT be presented as evidence of exact playback.

#### Scenario: Exact playback success
- **WHEN** a compatible event sequence and deterministic inputs are played back
- **THEN** the daemon MUST re-apply the transitions, report matching results, and record an exact-playback result

#### Scenario: Exact playback divergence
- **WHEN** playback produces a different transition, state, or deterministic result
- **THEN** the daemon MUST report divergence and MUST NOT claim exact replay success

#### Scenario: Simulation labeling
- **WHEN** a caller requests captured-outcome simulation instead of transition playback
- **THEN** the daemon MUST label and record the result as simulation and MUST reject its use as exact-playback evidence

### Requirement: Guarded crash reconciliation and recovery
After a crash or interrupted operation, reconciliation MUST compare durable events, projection generation, expected filesystem artifact identity and hash, and relevant Git state before recovery. The daemon MUST record discrepancies and recovery actions. A mismatch MUST prevent an unsafe repair; an ambiguous state MUST block recovery for explicit resolution. A safe recovery MUST be idempotent, MUST preserve authored content unless ownership, hash, lineage, and idempotency checks authorize the action, and MUST remain retry-safe.

#### Scenario: Clean reconciliation
- **WHEN** events, projection generation, filesystem artifact/hash, and relevant Git state agree
- **THEN** reconciliation MUST report a clean state and MUST NOT perform an unnecessary repair

#### Scenario: Reconciliation mismatch
- **WHEN** one compared durable surface differs from the others
- **THEN** the daemon MUST record the discrepancy, MUST not silently choose a surface as authoritative, and MUST require guarded recovery or resolution

#### Scenario: Ambiguous recovery
- **WHEN** ownership, hash, lineage, or operation identity cannot establish one safe recovery action
- **THEN** recovery MUST block without deleting or replacing authored content

#### Scenario: Idempotent safe recovery
- **WHEN** reconciliation establishes a uniquely safe recovery action and that action is retried
- **THEN** the daemon MUST apply it at most once, record the action and outcome append-only, and return the same durable result on retry

