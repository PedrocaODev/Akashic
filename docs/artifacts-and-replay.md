# Artifacts and replay

Authored Markdown contains human-owned intent, plans, decisions, contracts, and retrospectives. Structured runtime records contain append-only events, projections, findings, evidence, history, and replay metadata. Neither silently replaces the other.

An explicit import/version operation is required to bring an authored artifact into runtime state. It records artifact identity, content hash, author/source, schema version, and operation lineage. Ownership conflicts stop the operation for resolution; they are not auto-merged.

Events and schemas have versions. Migrations are explicit, reviewable, and preserve source lineage. Projections identify their generation and can be rebuilt from compatible events. An exact replay claim requires retained, compatible-input playback evidence that re-executes the recorded event sequence and timing-independent deterministic transitions; captured-outcome simulation merely replays recorded outcomes and cannot support an exact replay claim. A crash reconciliation compares durable events, projections, filesystem state, and Git state, records discrepancies, and requires guarded recovery.

Export, retention, and deletion are future boundaries; they are not implemented
by this change. End-to-end daemon boundary wiring is likewise deferred.
