## 1. Commands and exact outputs
- [x] 1.1 Write failing tests for exact commands, usage errors, version output/no daemon access, and minimal daemon/JSONL/TUI behavior.
- [x] 1.2 Write failing tests for exact doctor JSON schema, status enums, check fields, and zero/nonzero exit mapping.
- [x] 1.3 Implement commands and pass tests, including stdout/stderr discipline.

## 2. Configuration
- [x] 2.1 Write failing tests for XDG user path, nearest-Git-root-or-cwd project path, precedence, version, exact keys/defaults, environment names, CLI flags, types, and ranges.
- [x] 2.2 Write failing tests for unknown keys, raw secret fields, and case-insensitive secret-like classification.
- [x] 2.3 Implement config loading and pass tests.

## 3. Errors, redaction, and signals
- [x] 3.1 Write failing tests for exact error fields, locked codes, UUID correlation IDs, and retryability.
- [x] 3.2 Write failing tests for CLI/startup stderr-only serialization, JSONL `error.response`, destinations, and exit mappings.
- [x] 3.3 Write failing tests for first/second signals, 130/143, one timeout error, 124, and clean zero exit.
- [x] 3.4 Write failing tests proving redaction of classified names, broker values, and raw environment/config contents.
- [x] 3.5 Implement errors, destinations, redaction, and bounded shutdown and pass tests.

## 4. Socket and lock security
- [x] 4.1 Write failing tests for runtime/fallback paths, root-owned trusted ancestors, current-UID base/child, no-follow components, umask, and 0700/0600 modes.
- [x] 4.2 Write failing race tests for lock-before-inspect, active lock no-touch, active conflict, stale evidence, inode/device recheck, and cleanup.
- [x] 4.3 Write failing Linux tests for peer UID authorization before handshake parsing.
- [x] 4.4 Implement paths, lock lifecycle, stale handling, cleanup, and peer authorization and pass tests.

## 5. Protocol and JSONL
- [x] 5.1 Write failing tests for `akashic.local`, version 1, exact envelope fields/types, UUIDs, one-of payload/error, and allowed kinds.
- [x] 5.2 Write failing tests for handshake fields/roles/semver/exact capabilities and malformed/unsupported errors.
- [x] 5.3 Write failing tests for first-frame handshake sequencing, health-before-handshake rejection, new response IDs, request correlation IDs, and canonical objects.
- [x] 5.4 Write failing tests for UTF-8 one-object-per-line framing, inclusive 1048576-byte limit, malformed/oversized no-effect behavior, and stream separation.
- [x] 5.5 Write failing tests for `{}` health, exact `ok` response fields, idempotence, and TUI health/version display.
- [x] 5.6 Implement handshake, health, framing, limits, sequencing, identities, and streams and pass tests.

## 6. Quality and acceptance preparation
- [x] 6.1 Add CI checks for formatting, linting, tests, and documentation references.
- [x] 6.2 Run independent review, fix findings, and rerun affected checks.
- [x] 6.3 Inspect `openspec show bootstrap-rust-harness --json` and confirm exact constants and complete parsed clauses.
- [x] 6.4 Run strict validation and record exact implementation, socket, protocol, and output checks in `verify.md`.
- [x] 6.5 Reconcile documentation, complete `retrospective.md` before human acceptance/archive, and request acceptance.

## 7. Final contract coverage
- [x] 7.1 Add failing tests for doctor no-daemon `degraded`/`lifecycle.daemon_unavailable`/exit 4, successful health `ok`, and invalid config/path/socket/handshake `error`/exit 1.
- [x] 7.2 Add failing tests for invalid syntax before doctor selection producing generic stderr `usage.invalid`, and post-selection doctor config/runtime/socket/handshake outcomes producing exactly one stdout doctor result.
- [x] 7.3 Add failing tests for exact log levels, JSONL stdout versus authenticated socket error transport, and daemon stdout non-use.
- [x] 7.4 Add failing tests for root-owned readable/executable ancestor acceptance and unsafe ancestor/base/child cases.
- [x] 7.5 Add failing tests for mixed second signals, exact 130/143 behavior, dynamic illustrative UUID/semver values, new response/error IDs, and exact correlation relationships.
