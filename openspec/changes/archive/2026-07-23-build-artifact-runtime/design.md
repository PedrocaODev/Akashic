## Context

This change depends on the accepted and archived `bootstrap-rust-harness` and
extends its daemon/runtime foundation. Akashic is local-first: the deterministic
Rust runtime owns state, policy, approvals, evidence, effects, scheduling, and
replay, while LLM output is only proposed strategy. Authored Markdown remains
the human-readable canonical record; structured runtime state belongs in an
append-only SQLite event log and its deterministic projections.

The runtime must bridge three independently durable surfaces: authored files,
runtime records, and Git state. Import is therefore an explicit operation, not
an implicit scan or rewrite. Existing files may be owned by a person or another
runtime identity, and an ownership conflict must stop for resolution rather
than merge silently. Artifact identity, content hashes, schema versions, and
operation lineage must make every accepted import/version relationship
auditable without making SQLite the canonical authored content.

The design must also distinguish exact event playback from captured-outcome
simulation. Playback re-applies a recorded event sequence through deterministic,
timing-independent transitions; simulation only reuses recorded outcomes and
cannot establish that the same transitions would occur. A crash may leave the
database, projections, files, or Git at different points, so reconciliation
must compare them, preserve evidence of discrepancies, and recover only under
guarded rules. Export, retention, and deletion are not implemented by this
change.

## Goals / Non-Goals

**Goals:**

- Give the daemon ownership of artifact identity, explicit Markdown import,
  content-addressed version lineage, and operation provenance.
- Keep Markdown canonical for authored content and prevent runtime operations
  from silently rewriting or replacing authored history.
- Persist an append-only event history in SQLite and derive deterministic,
  identifiable projections for runtime state, findings, evidence, history, and
  replay metadata.
- Define explicit, reviewable schema/migration boundaries without assuming
  that a future migration can reinterpret incompatible historical data.
- Support rebuildable projections from compatible events and expose drift or
  incompleteness instead of presenting a rebuilt projection as unquestionable.
- Provide deterministic exact event playback as a separate capability and
  result from captured-outcome simulation.
- Reconcile database records, projections, filesystem artifacts, and Git after
  interruption; make recovery idempotent, guarded, and discrepancy-recording.
- Make import/version operations idempotent.
- Leave precise behavioral guarantees, formats, schemas, event vocabulary, and
  numeric policy thresholds for the normative specs and implementation tasks.

**Non-Goals:**

- Provider integration, credentials or provider policy.
- Sandbox, worktree, or substantive task execution.
- Agent lifecycle, convergence, or adaptive learning.
- A public plugin ABI or substantive TUI behavior.
- Arbitrary automatic merging or silent conflict resolution.
- Telemetry policy implementation.
- Export, retention, deletion, and end-to-end daemon integration/boundary wiring.
- Choosing final table schemas, event names, numeric retention defaults, or
  implementation crates in this design.

## Decisions

### Decision: The daemon is the authority for runtime ownership, not the filesystem or SQLite alone

Import and version operations go through daemon-controlled checks and produce
durable lineage. The filesystem remains the canonical location for authored
Markdown, while SQLite is authoritative for runtime events and projections.
Neither surface silently supersedes the other. This prevents an external file
change, stale projection, or duplicated request from becoming an unrecorded
state transition.

### Decision: Artifact identity is stable; versions are immutable and hash-bound

An artifact identity represents the logical authored record, while each
accepted content version is immutable and identified by its exact content hash
plus its recorded ancestry and import/version operation. The hash is computed
from the canonical content representation selected by the specs, and is
carried into any approval or evidence that depends on that content. A changed
file therefore creates a new candidate/version operation rather than mutating
an earlier version.

### Decision: Ownership conflicts fail closed and remain actionable

The daemon must detect conflicting ownership or incompatible lineage before
accepting an import/version operation. It records the attempted operation and
conflict context, then requires an explicit resolution; it does not auto-merge,
claim ownership, or rewrite the file. This favors a visible blocked operation
over data loss or an ambiguous canonical record.

### Decision: Idempotency is based on operation identity and content identity

Retries of the same import/version request must converge on one recorded
result, while a distinct operation with identical content must retain its own
provenance when that distinction matters. Replays and crash recovery use the
same durable identity checks, so retrying after an uncertain commit cannot
duplicate a version, append a second transition, or overwrite a newer file.

### Decision: Events are append-only; projections are disposable, versioned views

The event log is the durable transition history and is never updated in place
to repair a projection. Projections carry enough generation/schema context to
show which event history produced them. A rebuild reads compatible events in a
defined order, writes a replacement projection only after successful
reconstruction, and records failure or drift rather than masking it. Findings,
evidence, history, and replay metadata follow the same append-only boundary.

### Decision: Schema evolution is explicit and compatibility is tested at the boundary

Each persisted format has an explicit schema version and a reviewable migration
path. Startup or replay must refuse ambiguous or unsupported versions rather
than guessing. Migrations preserve source lineage and make compatibility
decisions observable; destructive reinterpretation of historical events is not
an acceptable migration shortcut. Exact migration ordering and formats belong
in the specs.

### Decision: Exact playback and captured-outcome simulation are separate contracts

Exact playback consumes the recorded event sequence and deterministic inputs,
re-applies runtime transitions without depending on wall-clock timing, and
reports divergence rather than silently accepting a different result. Captured-
outcome simulation consumes recorded outcomes for analysis or presentation and
does not claim to reproduce transitions. The runtime must label and store these
paths distinctly so a simulation cannot be used as evidence of exact replay.

### Decision: Crash recovery reconciles before repairing and never guesses over durable data

After an interruption, reconciliation compares the event log, projection
generation, expected filesystem content/hash, and relevant Git state. It
records every discrepancy and only applies a recovery action when ownership,
hash, lineage, and idempotency checks make the action safe. Ambiguous states
remain blocked for explicit resolution. Recovery is retry-safe and must not
delete or replace authored content merely because one durable surface is ahead.

## Risks / Trade-offs

- **Split-brain surfaces:** Filesystem or Git changes can race with daemon
  operations. Hashes, ownership checks, guarded reconciliation, and blocked
  ambiguity reduce damage at the cost of manual recovery.
- **Append-only growth:** Keeping event history increases local storage. Export,
  retention, and deletion policy are deferred to a follow-up.
- **Migration burden:** Versioned events and explicit compatibility make schema
  evolution slower than ad hoc changes, but avoid unreproducible replays and
  corrupted historical meaning.
- **Projection rebuild cost:** Rebuilding may be expensive and can temporarily
  leave a projection unavailable. That trade-off is accepted because derived
  state must not become a second source of truth.
- **Replay limits:** Exact playback proves deterministic transition behavior,
  not external-world reproduction; captured-outcome simulation is cheaper but
  intentionally weaker and must be labeled as such.

## Deferred follow-up

This non-normative scope is intentionally deferred: scoped export, retention
policy, deletion with descendant invalidation and secret-safe lineage, and
end-to-end daemon integration/boundary wiring.
