# ADR 0010: Modular monolith and dependency direction

**Status:** Accepted

Akashic remains one executable and one Rust package, organized as a modular
monolith. The deterministic runtime remains authoritative; this decision only
defines source boundaries and dependency direction. Normative product behavior
remains in OpenSpec.

## Decision

The target source organization is:

```text
src/
  main.rs                 # composition only
  lib.rs                  # real crate and test boundary
  artifacts/
    mod.rs                # narrow facade
    store.rs              # concrete Store and SQLite boundary
    schema.rs             # DDL, migrations, validation
    lineage.rs
    replay.rs
    recovery.rs
  config.rs runtime.rs json.rs shutdown.rs
```

Dependencies point inward toward capability ownership: composition may assemble
capabilities, but capability modules do not depend on `main`, CLI/protocol
adapters, or each other’s private implementation details. `artifacts::mod` is a
small private/`pub(crate)` facade, not a public Rust SDK. Executable-level
external contract tests use the binary; crate-private capability behavior uses
unit tests in or under its owning module. A public Rust API would require
approval. `Store` is concrete. `schema.rs` owns DDL, migrations, and physical
schema validation; `store.rs` owns connection lifecycle and transactions and
invokes schema operations. Capability-owned transactional statements may
remain with their capability, but no code outside `schema.rs` mutates the
schema. Source events and history are append-only authority; projections are
replaceable derived state.

## Non-decisions

This does not introduce a workspace or multiple crates, repository traits,
dependency injection, factories, ORMs, a generic service layer, one-type files,
or empty future modules without demonstrated need. It does not create premature
extension APIs or weaken the single-executable daemon authority.

## Adoption

Finish and reconcile the approved Slice 4 implementation at its current
location first. Until its checkpoint, only approved Slice 4 work may continue
in `src/artifacts.rs`; unrelated capabilities and extraction wait. After the
checkpoint, perform a behavior-preserving mechanical extraction into the target
modules. Future capability implementation starts in capability modules. The
`main.rs` composition split is a separate approved change.
