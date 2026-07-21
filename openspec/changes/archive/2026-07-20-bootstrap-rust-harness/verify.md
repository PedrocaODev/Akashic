# Verification status: ACCEPTED WITH WAIVERS

The bootstrap implementation passed local verification and hosted CI. Human
acceptance is complete with the documented waiver: cross-UID peer rejection
was conditionally skipped when `setpriv` could not perform UID switching.
Explicit delivery remains a separate boundary.

## Retained parsed requirements evidence

- Exact command output is retained at
  `openspec/changes/archive/2026-07-20-bootstrap-rust-harness/parsed-requirements.json` from
  `openspec show bootstrap-rust-harness --json`.
- `python3 scripts/check_docs.py` parses that artifact and checks complete
  requirement bodies, nonempty scenarios, explicit WHEN/THEN clauses, and
  `akashic.local`, version/line-size constants, `SO_PEERCRED`, and locked error
  constants.
- Direct artifact assertion passed: 22 complete requirement bodies, 48
  WHEN/THEN scenarios, locked constants present.
- `python3 -m unittest scripts/test_check_docs.py` passed and rejects truncated
  requirement/scenario input.

## Executed final local evidence

- `uname -a` — `Linux p-ernesto02 6.18.33.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun 18 21:54:43 UTC 2026 x86_64 x86_64 x86_64 GNU/Linux`.
- `cargo fmt -- --check` — passed.
- `cargo clippy --all-targets -- -D warnings` — passed.
- `cargo test --all-targets` — passed: 79 tests across 7 suites.
- `cargo test --test protocol_contracts` — passed: 12 tests.
- `cargo test --test signal_contracts --test security_contracts` — passed: 18 tests.
- `openspec validate --all --strict --no-interactive` — passed: 1 change.
- `python3 -m unittest scripts/test_check_docs.py` — passed: 1 test.
- `python3 scripts/check_docs.py` — passed: 38 Markdown files, parsed artifact,
  and untracked-file trailing-whitespace coverage.
- Secret/redaction scan over source, docs, README, and scripts — 0 matches for
  known fixture secrets/raw credential assignments.
- `git diff --check` — passed; untracked intended files are covered by the
  project documentation check's whitespace scan.

## Hosted CI

- Hosted workflow passed: [run 29786300215](https://github.com/PedrocaODev/Akashic/actions/runs/29786300215).

## Qualification

- Secure runtime, Unix locking, `SO_PEERCRED`, signal handling, and the secure
  protocol boundary are Linux-only and explicitly unsupported on non-Linux
  targets.
- WSL2 Linux-path tests passed. The `setpriv --reuid=65534 --regid=65534
  --clear-groups true` peer-UID qualification exited 127 with `setresuid
  failed: Operation not permitted`; the unauthorized-peer test skips only this
  environment limitation and makes no success claim for that branch.
- No providers, task execution, sandbox, storage/replay, or non-Linux fallback
  behavior is claimed by this bootstrap.

## Acceptance boundaries

- Human acceptance is complete with the documented cross-UID waiver.
- Explicit delivery remains separate and has not been performed.
