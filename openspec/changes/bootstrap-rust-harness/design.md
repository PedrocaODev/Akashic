## Context

Akashic is pre-implementation. The bootstrap must establish stable process and
runtime contracts without prematurely implementing dependent subsystems.

## Goals / Non-Goals

**Goals:** Provide one executable with daemon, TUI, JSONL, doctor, and version
modes; define deterministic configuration, errors, redaction, cancellation,
shutdown, and minimal local IPC seams; make tests and CI runnable.

**Non-Goals:** Providers, SQLite artifact runtime, production sandboxing,
worktrees, agent lifecycle, substantive TUI, Graphify/LSP, memory/skills,
Headroom, and public extension APIs.

## Decisions

- Explicit modes keep the executable contract discoverable and prevent silent
  fallback; the exact obligations are in `project-foundation/spec.md`.
- Layered configuration gives local operators predictable overrides while
  versioning and credential exclusion limit accidental secret handling; the
  exact precedence and failure rules are in `project-foundation/spec.md`.
- A per-user Unix socket and a small versioned JSONL envelope provide a local,
  inspectable seam without introducing a task protocol; fixed `akashic.local`
  version 1 constants and a health-only request keep the bootstrap surface
  intentionally small. Security and framing obligations are in
  `runtime-contracts/spec.md`.
- Lock-before-inspect, no-follow validation, inode/device rechecks, and peer-UID
  authorization address startup and cleanup races at the socket boundary.
- Clock and ID seams are justified only by deterministic tests. A provider seam
  is reserved for later phases and does not imply a provider implementation.
- Redaction and bounded shutdown are kept in the runtime contract because they
  protect operators and process state across every mode.

## Risks / Trade-offs

The minimal envelope may require additive versioning later. Keeping dependent
systems out of bootstrap reduces coupling but defers end-to-end behavior to the
roadmap phases.
