# Source guidance

- Keep the project a single-package modular monolith. Do not add new
  non-composition responsibilities to `main.rs`; composition-only is a target
  after a separately approved future split. Capabilities must not depend on it,
  CLI/protocol code, or private details of sibling capabilities.
- Only approved Slice 4 work may continue in `src/artifacts.rs` until its
  checkpoint. Unrelated capabilities and extraction wait; afterward use
  `src/artifacts/` modules for mechanical, behavior-preserving extraction.
- `schema.rs` is the sole owner of DDL, migrations, and physical schema
  validation. `store.rs` owns connection lifecycle and transactions and invokes
  schema operations; capability-owned transactional statements may remain with
  their capability. `Store` is concrete and no direct schema mutation occurs
  outside `schema.rs`.
- Keep a small `artifacts` re-export facade. Distinguish append-only source
  events/history from replaceable projections.
- Do not add speculative modules, traits, factories, or module-wide
  `dead_code` suppression. Route schema/migration work to schema review and
  recovery/filesystem identity work to security review.
