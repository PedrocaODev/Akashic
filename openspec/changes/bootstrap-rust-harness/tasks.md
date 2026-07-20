## 1. Commands and exact outputs
- [ ] 1.1 Write failing tests for exact commands, usage errors, version output/no daemon access, and minimal daemon/JSONL/TUI behavior.
- [ ] 1.2 Write failing tests for exact doctor JSON schema, status enums, check fields, and zero/nonzero exit mapping.
- [ ] 1.3 Implement commands and pass tests, including stdout/stderr discipline.

## 2. Configuration
- [ ] 2.1 Write failing tests for XDG user path, nearest-Git-root-or-cwd project path, precedence, version, exact keys/defaults, environment names, CLI flags, types, and ranges.
- [ ] 2.2 Write failing tests for unknown keys, raw secret fields, and case-insensitive secret-like classification.
- [ ] 2.3 Implement config loading and pass tests.

## 3. Errors, redaction, and signals
- [ ] 3.1 Write failing tests for exact error fields, locked codes, UUID correlation IDs, and retryability.
- [ ] 3.2 Write failing tests for CLI/startup stderr-only serialization, JSONL `error.response`, destinations, and exit mappings.
- [ ] 3.3 Write failing tests for first/second signals, 130/143, one timeout error, 124, and clean zero exit.
- [ ] 3.4 Write failing tests proving redaction of classified names, broker values, and raw environment/config contents.
- [ ] 3.5 Implement errors, destinations, redaction, and bounded shutdown and pass tests.

## 4. Socket and lock security
- [ ] 4.1 Write failing tests for runtime/fallback paths, root-owned trusted ancestors, current-UID base/child, no-follow components, umask, and 0700/0600 modes.
- [ ] 4.2 Write failing race tests for lock-before-inspect, active lock no-touch, active conflict, stale evidence, inode/device recheck, and cleanup.
- [ ] 4.3 Write failing Linux tests for peer UID authorization before handshake parsing.
- [ ] 4.4 Implement paths, lock lifecycle, stale handling, cleanup, and peer authorization and pass tests.

## 5. Protocol and JSONL
- [ ] 5.1 Write failing tests for `akashic.local`, version 1, exact envelope fields/types, UUIDs, one-of payload/error, and allowed kinds.
- [ ] 5.2 Write failing tests for handshake fields/roles/semver/exact capabilities and malformed/unsupported errors.
- [ ] 5.3 Write failing tests for first-frame handshake sequencing, health-before-handshake rejection, new response IDs, request correlation IDs, and canonical objects.
- [ ] 5.4 Write failing tests for UTF-8 one-object-per-line framing, inclusive 1048576-byte limit, malformed/oversized no-effect behavior, and stream separation.
- [ ] 5.5 Write failing tests for `{}` health, exact `ok` response fields, idempotence, and TUI health/version display.
- [ ] 5.6 Implement handshake, health, framing, limits, sequencing, identities, and streams and pass tests.

## 6. Quality and acceptance preparation
- [ ] 6.1 Add CI checks for formatting, linting, tests, and documentation references.
- [ ] 6.2 Run independent review, fix findings, and rerun affected checks.
- [ ] 6.3 Inspect `openspec show bootstrap-rust-harness --json` and confirm exact constants and complete parsed clauses.
- [ ] 6.4 Run strict validation and record exact implementation, socket, protocol, and output checks in `verify.md`.
- [ ] 6.5 Reconcile documentation, complete `retrospective.md` before human acceptance/archive, and request acceptance.

## 7. Final contract coverage
- [ ] 7.1 Add failing tests for doctor no-daemon `degraded`/`lifecycle.daemon_unavailable`/exit 4, successful health `ok`, and invalid config/path/socket/handshake `error`/exit 1.
- [ ] 7.2 Add failing tests for invalid syntax before doctor selection producing generic stderr `usage.invalid`, and post-selection doctor config/runtime/socket/handshake outcomes producing exactly one stdout doctor result.
- [ ] 7.2 Add failing tests for exact log levels, JSONL stdout versus authenticated socket error transport, and daemon stdout non-use.
- [ ] 7.3 Add failing tests for root-owned readable/executable ancestor acceptance and unsafe ancestor/base/child cases.
- [ ] 7.4 Add failing tests for mixed second signals, exact 130/143 behavior, dynamic illustrative UUID/semver values, new response/error IDs, and exact correlation relationships.
