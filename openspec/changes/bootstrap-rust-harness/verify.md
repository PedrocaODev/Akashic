# Verification status: PENDING

No implementation evidence exists yet. Do not treat this change as verified.

## Current planning validation

- `openspec validate --all --strict --no-interactive` passed for the active
  change at planning time.
- `openspec show bootstrap-rust-harness --json` was inspected after this
  revision; every parsed requirement body was complete and no body was a
  truncated prefix. This is planning validation, not implementation evidence.

## Intended evidence path

- Run strict OpenSpec validation for this change.
- Inspect the exact parsed JSON with `openspec show bootstrap-rust-harness --json`
  and retain the relevant output or a checked artifact showing complete clauses.
- Confirm parsed JSON contains `akashic.local`, protocol version `1`, exact
  envelope/kind/handshake/health constants, 1048576-byte line limit, exact
  config paths/defaults/overrides, locked error codes, doctor schema, response
  identity/correlation, error destinations/exit mappings, signal behavior, and
  socket race rules including root-owned trusted ancestors.
- Verify exact log-level enum, doctor no-daemon degraded result and health/error
  paths, distinct CLI/JSONL/socket error destinations, mixed second-signal
  exits, and dynamic example IDs with exact correlation relationships.
- Verify invalid syntax before doctor selection uses generic stderr usage JSON,
  while every post-selection doctor outcome uses exactly one stdout doctor
  result with redacted diagnostics only on stderr.
- After implementation, run formatting, lint, unit/integration tests, and CI
  checks recorded with exact commands and outputs.
- Exercise supported/invalid modes, configuration precedence, redaction,
  cancellation, socket handshake compatibility, and malformed JSONL envelopes.
- Exercise bootstrap security and protocol checks: XDG/fallback permissions,
  symlink and ownership rejection, singleton/stale behavior, no active unlink,
  Linux peer UID authorization, exact handshake fields, UTF-8/newline framing,
  1 MiB oversize rejection, no-effect failures, ping idempotence, and stdout/
  stderr separation.
- Exercise exact command output and exit checks for version, doctor, daemon,
  JSONL, and TUI, including signals and second-signal behavior.
- Exercise exact command output and exit checks for version, doctor, daemon,
  JSONL, and TUI, including signals and second-signal behavior.
- Record review/fix rerun results and link each result to the relevant task and
  requirement scenario.
