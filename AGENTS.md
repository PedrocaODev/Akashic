# Akashic project guidance

## Purpose

Akashic is an open-source, local-first Rust AI coding harness for Linux, WSL2,
and headless Linux. Only the bootstrap executable baseline is implemented;
dependent runtime systems remain future work.

## Invariants

- The deterministic Rust runtime owns state, policy, approvals, evidence, scheduling, and invariants; LLMs propose strategy only.
- Canonical authored artifacts are Markdown. SQLite's append-only source events, evidence, and history are authoritative; projections are replaceable, rebuildable derived state and are not authority.
- Every task has a task integration worktree. Logical child writer worktrees are sibling directories; the daemon owns Git integration and ephemeral commits.
- Native execution is sandboxed with explicit security levels and no silent fallback. Docker, network, display, SSH, and D-Bus are denied by default.
- Credentials are official provider API/service/workload credentials only; CLI subscription tokens are never imported.
- Terminal outcomes distinguish verified, accepted_with_waivers, accepted_partial, blocked, aborted, and failed.
- Normative behavior belongs in OpenSpec specs; these documents explain intent and boundaries.
- Keep the Rust project as a modular monolith: use the narrowest existing capability module, do not add new non-composition responsibilities to `main.rs`, and do not add premature crates, traits, factories, or public APIs. The composition-only `main.rs` shape is a target for a separately approved future split, not a reason for an unrelated immediate refactor. Separate mechanical moves from behavior changes.
- Approval is required for a new dependency, schema version or semantics, public API, security or destructive boundary, or dependency-direction reversal. Never weaken fail-closed, security, compatibility, or data-preservation checks.

## Paths and approval boundaries

- `docs/` contains durable explanatory design and decisions; `openspec/` contains normative requirements and is owned by the OpenSpec writer.
- Do not edit `openspec/**` without explicit ownership transfer.
- Human approval of an implementation plan authorizes its planned slices and bounded review/fix iterations. Re-approval is required for material scope expansion, a new dependency, schema version or semantics, public API, security or destructive boundary, dependency-direction reversal, new normative behavior, learning activation, elevated security, provider use, or explicit delivery. Human acceptance is required before delivery.

## Verification expectations

Documentation changes must have checked internal Markdown links and a clean diff review. Future implementation changes must provide risk-based evidence, exact event playback where claimed, and a mandatory retrospective.

## Specialist routing

Use repository exploration for broad discovery, documentation/research specialists for external references, security review for sandbox or credential changes, and verification review for lifecycle or evidence changes. Keep Rust implementation work separate from documentation-only changes.
