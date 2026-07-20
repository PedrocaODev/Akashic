# ADR 0001: Deterministic control plane

**Status:** Accepted

## Decision

The Rust runtime owns state, policy, approvals, evidence, scheduling, and invariants. The LLM orchestrator proposes strategy only. The fixed core is orchestrator, planner, implementer, and reviewer; scoped fresh fixers are bounded, and dynamic helpers are created only through the orchestrator.

## Consequences

Model output cannot directly authorize actions or rewrite history. Deterministic transitions and approval evidence are required, at the cost of more explicit runtime machinery. Normative details belong in OpenSpec.
