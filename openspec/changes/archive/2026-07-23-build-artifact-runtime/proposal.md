# Proposal: Build the artifact runtime

## Why

The accepted `bootstrap-rust-harness` provides the executable foundation, but
the daemon still lacks durable, reviewable runtime state. This milestone makes
authored Markdown artifacts and append-only runtime history coexist without
silently merging ownership, losing lineage, or claiming replay guarantees the
system cannot prove.

## What Changes

- Establish explicit import and versioning for owned Markdown artifacts,
  including identity, content, ownership, and operation lineage.
- Add append-only SQLite storage for versioned events, projections, findings,
  evidence, history, and replay metadata.
- Define explicit, reviewable migrations and rebuildable projections.
- Provide deterministic exact event replay, distinct from captured-outcome
  simulation.
- Add crash reconciliation across durable records, projections, filesystem
  state, and Git state, with guarded recovery and recorded discrepancies.
The active change ends at guarded crash reconciliation and recovery. Export,
retention, deletion, and end-to-end daemon integration remain future work.

## Capabilities (new/modified)

- **New:** owned Markdown artifact import/versioning and lineage.
- **New:** append-only event storage, schema migrations, projections, and
  deterministic replay.
- **New:** crash reconciliation and guarded recovery.
- **Modified:** runtime persistence and lifecycle evidence use the artifact
  runtime records where implemented through Slice 4.

## Impact

This depends on the accepted and archived `bootstrap-rust-harness` and extends
its daemon/runtime foundation. It establishes the storage and evidence boundary
needed by later milestones while preserving human-readable authored records.

Risks include ownership conflicts, migration or projection drift, and
incomplete crash recovery; these require explicit failure handling, lineage
records, and verification in the design and specification.

Scope excludes providers, sandboxing, worktrees, agent lifecycle, adaptive
learning, a public plugin ABI, and substantive task execution. Exact schemas,
retention durations, deletion policies, export behavior, and end-to-end daemon
boundary wiring are outside this change.

## Deferred follow-up

This non-normative follow-up scope is not implemented here and does not create
a new change directory:

- scoped export;
- retention policy;
- deletion, descendant invalidation, and secret-safe deletion lineage; and
- end-to-end daemon integration and boundary wiring.
