# Verification status: VERIFIED LOCALLY — ACCEPTANCE PENDING

The bootstrap implementation and local evidence path are verified below. This
does not claim hosted CI completion, human acceptance, or explicit delivery.

## Retained parsed requirements evidence

- Exact command output is retained at
  `openspec/changes/bootstrap-rust-harness/parsed-requirements.json` from
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

## Pending acceptance boundaries

- Hosted CI workflow execution has not been run in this environment.
- Human acceptance, explicit delivery, and archive/acceptance workflow remain
  pending.
