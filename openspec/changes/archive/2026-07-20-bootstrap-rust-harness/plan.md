# Implementation plan: bootstrap-rust-harness

## Execution rules

- Implement each slice in order, with the failing test written and observed
  before the corresponding production behavior.
- Keep the runtime contract authoritative: tests assert exact values, output
  destinations, exit codes, and security boundaries rather than implementation
  details.
- After each review checkpoint, fix or explicitly disposition every actionable
  finding and rerun the review before advancing.
- Do not commit evidence or documentation claims while the relevant tests are
  red. Record final verification results in `verify.md` only after the review
  loop is clean.

## Slice 1 — package and exact CLI mode contracts

**Task mapping:** 1.1–1.3; 7.1–7.2.

1. Add focused CLI/integration tests for package metadata, exact commands,
   version output without daemon access, usage errors, invalid/conflicting
   usage, stdout/stderr discipline, and the minimal daemon, JSONL, and TUI
   contracts.
2. Add failing tests for doctor schema/status/check fields and 0/4/1 exit
   mapping, including no-daemon degraded output and generic-versus-doctor
   output precedence.
3. Implement the package metadata and CLI modes, keeping unsupported dependent
   features absent rather than faked; make the tests pass.

**TDD exception:** Apache LICENSE and Cargo metadata portions may be verified
structurally where no behavioral failing test is meaningful. The exception is
limited to those artifacts; all CLI behavior remains test-first.

## Slice 2 — config resolution, doctor schema, errors, and redaction

**Task mapping:** 2.1–2.3; 3.1–3.2 and 3.4; 7.1–7.2.

1. Add failing tests for XDG user paths, nearest-Git-root-or-cwd project
   paths, precedence, version, exact keys/defaults, environment names, CLI
   flags, types, and ranges.
2. Add failing tests for unknown keys, raw secret fields, and case-insensitive
   secret-like classification.
3. Add failing tests for exact error fields, locked codes, UUID correlation
   IDs, retryability, destinations, exit mappings, CLI/startup stderr-only
   serialization, JSONL `error.response`, and secret redaction.
4. Add failing tests for doctor 0/4/1 output, generic invalid syntax before
   doctor selection, and exactly one post-selection doctor result.
5. Implement resolution, schema validation, doctor projection, errors,
   destinations, and redaction; make all slice tests pass.

**TDD exception:** none.

**Review checkpoint:** perform contract/API/config/error review; fix or
explicitly disposition every finding and rerun the review and affected tests.

## Slice 3 — signals and bounded shutdown

**Task mapping:** 3.3, 3.5, and 7.5.

1. Add failing tests for first SIGINT/SIGTERM, mixed second signals with exact
   130/143 behavior, child cancellation, timeout/error/exit behavior, one
   timeout error, 124, and clean zero exit.
2. Implement bounded shutdown and signal propagation; make the tests pass.

**TDD exception:** only a platform-specific signal case that cannot run in
ordinary unit CI may use a documented integration/manual qualification path.
The Linux signal path must remain automated.

## Slice 4 — secure daemon socket and singleton

**Task mapping:** 4.1–4.4 and 7.4.

1. Add failing tests for runtime/fallback paths, root/current-UID ancestor
   rules, private base permissions, no-follow/symlink rejection, umask, and
   0700/0600 modes.
2. Add failing race tests for lock-before-inspect, active/stale socket races,
   active conflict, stale evidence, inode/device recheck, cleanup ownership,
   and lock lifecycle.
3. Add failing Linux tests proving SO_PEERCRED authorization before handshake
   parsing.
4. Implement paths, lock lifecycle, stale handling, cleanup, and peer
   authorization; make the tests pass.

**TDD exception:** unsupported non-Linux hosts may be structurally or manually
qualified, but the Linux implementation and tests are mandatory and automated.

**Review checkpoint:** perform security review of path, lock, peer, and signal
behavior; fix or disposition every finding and rerun the review and affected
tests.

## Slice 5 — versioned handshake, health, framing, and JSONL discipline

**Task mapping:** 5.1–5.6 plus 7.3 and 7.5.

1. Add failing tests for exact envelope fields/types/constants, `akashic.local`,
   version 1, UUIDs, one-of payload/error, allowed kinds, handshake fields,
   roles, semver, and exact capabilities.
2. Add failing tests for handshake-before-health sequencing, malformed and
   unsupported errors, dynamic IDs/correlation relationships, illustrative
   examples, and new response/error IDs.
3. Add failing tests for UTF-8 one-object-per-line framing, the inclusive
   1 MiB limit, malformed/oversized no-effect behavior, health idempotence,
   and canonical `{}`/`ok` responses.
4. Add failing tests for daemon-socket versus `run --jsonl` error transport,
   exact log levels, stdout/stderr separation, daemon stdout non-use, and TUI
   health/version display.
5. Implement handshake, health, framing, limits, sequencing, identities, and
   streams; make all tests pass.

**TDD exception:** none.

**Review checkpoint:** perform protocol/replayability review; fix or
explicitly disposition every finding and rerun the review and affected tests.

## Slice 6 — CI and documentation/evidence closure

**Task mapping:** 6.1–6.5.

1. Add or update checks for formatting, linting, tests, documentation
   references, strict OpenSpec validation, parsed bootstrap-requirements
   completeness, and Markdown relative links.
2. Run independent review, fix or disposition all findings, and rerun the
   affected checks. Reconcile documentation and prepare retrospective and
   acceptance artifacts without fabricating evidence.
3. Record exact implementation, socket, protocol, output, and qualification
   results in `verify.md`.

**TDD exception:** CI workflow and document-only wiring have no meaningful
   failing behavioral test. Replace TDD with schema validation, Markdown link
   checking, and a deliberately failing CI dry run or equivalent configuration
   validation when feasible. Behavioral checks remain test-first.

**Review checkpoint:** perform a final independent review; all findings must
be fixed or explicitly dispositioned and the reviewer rerun clean before final
verification.

## Final verification intent

Only after the final review is clean, run and record:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test` (use `cargo nextest run` only if bootstrap explicitly installs
  and uses it)
- strict OpenSpec validation
- parsed JSON completeness check for bootstrap requirements
- repository Markdown relative-link check
- Linux secure-socket and signal integration tests
- documented WSL2 smoke qualification if no automated WSL runner exists
- secret/redaction scan and clean `git diff --check`

## Intended commit grouping

Do not create commits during plan generation. Once tests are green and the
corresponding review loop is clean, group changes logically:

1. package and CLI;
2. config, errors, redaction, and shutdown;
3. secure socket and protocol;
4. CI, docs, and evidence.

No evidence commit precedes green tests for the behavior it claims.
