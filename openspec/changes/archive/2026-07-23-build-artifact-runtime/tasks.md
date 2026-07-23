# Tasks: Build the artifact runtime

## 1. Storage foundation and compatibility boundary

- [x] 1.1 Add a focused failing test that opens a fresh runtime store, verifies the SQLite connection is usable, and checks that schema-version metadata is recorded.
- [x] 1.2 Implement the runtime storage foundation and SQLite connection initialization, including schema-version metadata and the existing Rust module layout.
- [x] 1.3 Add a focused failing test for startup with a supported older schema and with an unsupported or ambiguous schema version; assert migration or fail-closed rejection respectively.
- [x] 1.4 Implement explicit, ordered, reviewable migrations that preserve source lineage and record the compatibility decision without destructively reinterpreting historical records.

## 2. Artifact identity, canonical Markdown, and lineage

- [x] 2.1 Add failing tests for first explicit import and for rejection of filesystem discovery or implicit scanning as an accepted version (artifact-lineage: daemon-owned identity and explicit versioning).
- [x] 2.2 Implement artifact identity creation and explicit canonical Markdown import, recording schema version, source, author, ancestry, operation identity, and operation lineage.
- [x] 2.3 Add failing tests for canonical content hashing, immutable accepted versions, changed-content versioning, and preservation of the prior version (artifact-lineage: immutable hash-bound versions).
- [x] 2.4 Implement canonical Markdown representation, content hashing, immutable version records, and ancestry-linked candidate/version operations without selecting unapproved numeric or format policies.
- [x] 2.5 Add failing tests for repeated operation identity retries and distinct operations with identical content; assert the specified idempotent result and provenance behavior.
- [x] 2.6 Implement operation/content identity checks so retries converge without duplicate versions or transitions while distinct operations retain required lineage.
- [x] 2.7 Add failing tests for ownership conflict, incompatible lineage, and external file drift; assert fail-closed behavior, actionable non-secret context, and no file rewrite or auto-merge.
- [x] 2.8 Implement ownership, lineage, observed-file identity, and expected-hash checks with recorded conflict and drift discrepancies requiring explicit resolution.

## 3. Append-only runtime records and invariants

- [x] 3.1 Add failing tests proving that events, findings, evidence, history, and replay metadata carry schema versions and cannot be updated or deleted to repair prior history (event-replay-recovery: append-only durable runtime history).
- [x] 3.2 Implement append-only storage for events, findings, evidence, history, and replay metadata, including source lineage and invariant enforcement for all write paths.
- [x] 3.3 Add failing tests for lifecycle evidence and outcome records, including invalidated evidence, reviewer finding states, bounded terminal outcomes, retrospective ordering, and acceptance requirements described by the lifecycle contract.
- [x] 3.4 Implement the smallest runtime record transitions needed to persist those lifecycle/evidence invariants as new records rather than mutations.

## 4. Projections and generation metadata

- [x] 4.1 Add failing tests for missing, stale, and drifted projections, asserting event-history generation/schema identification and non-authoritative failure or incompleteness status (event-replay-recovery: deterministic projections and visible drift).
- [x] 4.2 Implement versioned projections with generation and schema metadata, deterministic rebuild ordering from compatible events, and replacement only after successful reconstruction.
- [x] 4.3 Add failing tests for successful deterministic rebuild and for rebuild failure, including proof that the old projection is not silently presented as authoritative.
- [x] 4.4 Implement visible rebuild status, drift/failure recording, and deterministic projection regeneration without changing append-only source records.

## 5. Exact playback and captured-outcome simulation

- [x] 5.1 Add failing tests for exact playback success and divergence using a compatible event sequence and deterministic inputs (event-replay-recovery: exact playback).
- [x] 5.2 Implement timing-independent exact playback that reapplies recorded transitions, reports divergence, and records the exact-playback result.
- [x] 5.3 Add a failing test proving captured-outcome simulation is labeled and stored separately and cannot be used as exact-playback evidence (event-replay-recovery: simulation labeling).
- [x] 5.4 Implement captured-outcome simulation as a distinct, explicitly labeled path with no exact replay claim.

## 6. Crash reconciliation and guarded recovery

- [x] 6.1 Add failing tests for clean reconciliation and for mismatches among events, projection generation, expected filesystem artifact/hash, and relevant Git state (event-replay-recovery: crash reconciliation).
- [x] 6.2 Implement reconciliation that compares all applicable durable surfaces, records every discrepancy, and performs no unnecessary repair or silent authority selection.
- [x] 6.3 Add failing tests for ambiguous recovery and for unsafe ownership/hash/lineage states; assert blocking without deleting or replacing authored content.
- [x] 6.4 Implement guarded recovery checks that allow only uniquely safe actions and preserve authored content when the state is ambiguous.
- [x] 6.5 Add a failing test for retrying a uniquely safe recovery action after an uncertain interruption; assert at-most-once application and the same durable result.
- [x] 6.6 Implement retry-idempotent recovery using durable operation identity, append-only recovery action/outcome records, and recorded discrepancies.

## Deferred follow-up (not active tasks)

Scoped export, retention policy, deletion/descendant invalidation/secret-safe
deletion lineage, and end-to-end daemon integration/boundary wiring are
deferred to future OpenSpec work. No unchecked tasks are claimed for them.

## 8. Review, verification, documentation, and retrospective

- [x] 8.5 Review and fix implementation/test issues against the artifact-lineage and event-replay-recovery contracts, preserving the narrowed scope and append-only invariants.
- [x] 8.6 Run Rust, documentation, OpenSpec, boundary, and diff verification and record the actual evidence in `verify.md`.
- [x] 8.7 Reconcile `docs/artifacts-and-replay.md` and `docs/privacy-and-retention.md` with implemented behavior and deferred boundaries.
- [x] 8.8 Prepare the retrospective and scope-decision evidence in `retrospective.md`, including unresolved risks and waivers.
