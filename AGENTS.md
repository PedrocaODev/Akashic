# Akashic project guidance

## Purpose

Akashic is a pre-implementation, open-source, local-first Rust AI coding harness for Linux, WSL2, and headless Linux.

## Invariants

- The deterministic Rust runtime owns state, policy, approvals, evidence, scheduling, and invariants; LLMs propose strategy only.
- Canonical authored artifacts are Markdown. SQLite is append-only for events, projections, findings, evidence, history, and replay.
- Every task has a task integration worktree. Logical child writer worktrees are sibling directories; the daemon owns Git integration and ephemeral commits.
- Native execution is sandboxed with explicit security levels and no silent fallback. Docker, network, display, SSH, and D-Bus are denied by default.
- Credentials are official provider API/service/workload credentials only; CLI subscription tokens are never imported.
- Terminal outcomes distinguish verified, accepted_with_waivers, accepted_partial, blocked, aborted, and failed.
- Normative behavior belongs in OpenSpec specs; these documents explain intent and boundaries.

## Paths and approval boundaries

- `docs/` contains durable explanatory design and decisions; `openspec/` contains normative requirements and is owned by the OpenSpec writer.
- Do not edit `openspec/**` without explicit ownership transfer.
- Human approval is required before implementation and human acceptance before delivery. Learning activation, elevated security, provider use, and explicit delivery are also approval boundaries.

## Verification expectations

Documentation changes must have checked internal Markdown links and a clean diff review. Future implementation changes must provide risk-based evidence, exact event playback where claimed, and a mandatory retrospective.

## Specialist routing

Use repository exploration for broad discovery, documentation/research specialists for external references, security review for sandbox or credential changes, and verification review for lifecycle or evidence changes. Keep Rust implementation work separate from documentation-only changes.
