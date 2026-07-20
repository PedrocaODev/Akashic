# ADR 0002: Artifact storage and replay

**Status:** Accepted

## Decision

Markdown is canonical for authored artifacts. SQLite is append-only for events, projections, findings, evidence, history, and replay. Raw task history remains local until explicit deletion. Exact event playback is a stronger claim than captured-outcome simulation and must never be conflated with it.

## Consequences

Files and runtime records have explicit ownership. Projections can be rebuilt from events, while authored Markdown remains legible and reviewable. Storage and replay guarantees require implementation tests and OpenSpec scenarios.
