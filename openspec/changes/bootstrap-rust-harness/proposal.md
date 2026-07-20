## Why

Akashic needs a small, deterministic executable foundation before dependent
runtime, security, provider, and client phases can be implemented.

depends-on: none

## What Changes

Create the Apache-licensed Cargo package and executable mode contracts,
configuration and error conventions, minimal daemon handshake, versioned JSONL
envelope, shutdown behavior, and a test/CI/documentation baseline.

Scope: package foundation, five command forms, configuration, structured errors,
redaction, lifecycle, secure bootstrap IPC, versioned JSONL framing, health,
tests, CI, and documentation references.

Apply readiness: exact doctor schema, error destinations/codes/exit mappings,
signal outcomes, trusted ancestor rules, lock races, handshake ordering,
response identity, dynamic illustrative examples, log-level enum, degraded
doctor behavior, and canonical protocol examples are locked in the specs.

## Capabilities

### New Capabilities
- `project-foundation`: package, modes, configuration, errors, logging, and lifecycle conventions.
- `runtime-contracts`: daemon handshake and versioned JSONL process envelope.

### Modified Capabilities
- None.

## Impact

This change creates the initial Rust package and project baseline. It does not
implement providers, artifact persistence, production sandboxing, worktrees,
agent lifecycle, substantive TUI, Graphify/LSP, memory/skills, Headroom, or
extension APIs.

## Non-Goals

Providers, SQLite artifact runtime, production sandboxing, worktrees, agent
lifecycle, substantive TUI, Graphify/LSP, memory/skills, Headroom, and public
extension APIs are deferred to dependent roadmap changes.

## Risks

The bootstrap protocol and configuration formats may need additive evolution;
strict versioning and fail-closed behavior trade convenience for safety. Secure
socket portability is limited to supported Linux primitives.

## Acceptance Criteria

- The package and command contracts are implemented with focused tests.
- Configuration, errors, redaction, cancellation, shutdown, socket security,
  singleton behavior, peer authorization, handshake, framing, and health have
  passing evidence.
- CI checks pass, review findings are fixed and rerun, and `verify.md` and
  `retrospective.md` are complete before human acceptance.
