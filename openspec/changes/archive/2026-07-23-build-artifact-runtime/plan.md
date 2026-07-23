# Implementation plan: build-artifact-runtime

## Execution rules

- Implement the slices in order. For every behavioral task, write and observe
  the failing test before implementing the behavior.
- Keep Markdown canonical for authored content and SQLite append-only for
  runtime history and derived projections. Do not claim verification evidence
  until the relevant checks have run.
- After each review checkpoint, fix or explicitly disposition every finding and
  rerun the affected tests before advancing.
- Preserve the approved non-goals: no providers, sandbox/worktrees, agent
  lifecycle, adaptive learning, plugin ABI, or substantive task execution.

## Slice 1 — storage foundation, migrations, and artifact lineage

**Task mapping:** 1.1–1.4 and 2.1–2.8.

1. Write failing tests for fresh-store initialization, schema metadata,
   supported migration, and fail-closed rejection of unsupported or ambiguous
   schemas.
2. Implement the storage foundation and explicit ordered migrations, preserving
   source lineage and recording compatibility decisions.
3. Write failing tests for explicit-only import, artifact identity, canonical
   hashing, immutable versions, changed-content ancestry, operation retry
   idempotency, distinct-operation provenance, ownership/lineage conflicts, and
   external file drift without rewrite or auto-merge.
4. Implement daemon-owned artifact identity, canonical Markdown import,
   hash-bound immutable versions, operation identity checks, and fail-closed
   ownership/drift handling.

**TDD exception:** none.

**Review checkpoint:** review storage/artifact lineage for append-only
invariants, schema/migration compatibility, ownership and security boundaries,
canonical-content and hash/lineage claims, idempotency, and scope creep. Fix or
disposition findings and rerun affected tests.

## Slice 2 — append-only records and lifecycle evidence

**Task mapping:** 3.1–3.4.

1. Write failing tests proving schema-versioned events, findings, evidence,
   history, and replay metadata cannot be updated or deleted, plus lifecycle
   evidence, reviewer findings, bounded terminal outcomes, retrospective
   ordering, and acceptance requirements.
2. Implement append-only durable records and the smallest required lifecycle
   transitions, using new records for corrections and discrepancies.

**TDD exception:** none.

## Slice 3 — projections, exact playback, and simulation

**Task mapping:** 4.1–4.4 and 5.1–5.4.

1. Write failing tests for missing/stale/drifted projections, deterministic
   rebuild success and failure, generation/schema metadata, and preservation of
   the old projection as non-authoritative on failure.
2. Implement versioned projections, deterministic rebuild ordering, visible
   drift/failure status, and replacement only after successful reconstruction.
3. Write failing tests for exact playback success and divergence, and for
   separately labeled captured-outcome simulation that cannot support an exact
   replay claim.
4. Implement timing-independent exact playback and the distinct simulation
   path, recording their results under separate contracts.

**TDD exception:** none.

**Review checkpoint:** review event/projection/replay behavior for append-only
source-of-truth invariants, generation/schema identification, deterministic
rebuild and replay claims, divergence handling, simulation labeling, and scope
creep. Fix or disposition findings and rerun affected tests.

## Slice 4 — reconciliation and guarded recovery

**Task mapping:** 6.1–6.6.

1. Write failing tests for clean reconciliation and mismatches among events,
   projection generation, expected filesystem identity/hash, and relevant Git
   state.
2. Implement comparison of all applicable durable surfaces, discrepancy
   recording, and no unnecessary or silent authority selection.
3. Write failing tests for ambiguous and unsafe ownership/hash/lineage states,
   asserting blocking without authored-content deletion or replacement, and for
   retrying a uniquely safe action with at-most-once durable results.
4. Implement guarded, retry-idempotent recovery with append-only action and
   outcome records.

**TDD exception:** none.

**Review checkpoint:** review reconciliation for ownership/security checks,
   preservation of authored content, discrepancy completeness, replay/evidence
   claims, idempotency, and scope creep. Fix or disposition findings and rerun
   affected tests.

## Deferred follow-up — export, retention, deletion, and daemon integration

Scoped export, retention policy, deletion/descendant invalidation/secret-safe
deletion lineage, and end-to-end daemon integration/boundary wiring are
deferred. No active implementation tasks are claimed for them.

## Slice 7 — implementation review and verification preparation

**Task mapping:** 8.5–8.6.

1. Write failing checks, where meaningful, for the focused Rust test targets,
   formatting/build/lint commands, strict OpenSpec validation, applicable
   schema validation, documentation/link checks, whitespace, and clean diff
   review.
2. Review and fix implementation/test issues against both contracts; run the
   targeted checks and record gaps without fabricating evidence.

**TDD exception:** none for behavioral checks. Tooling and review checks may
fail structurally rather than through a behavioral test; they must still be
executed and their results recorded.

## Slice 8 — documentation, retrospective, and acceptance evidence

**Task mapping:** 8.7–8.8.

1. Reconcile `docs/artifacts-and-replay.md`, `docs/lifecycle-and-outcomes.md`,
   and `docs/privacy-and-retention.md` with implemented behavior, without
   changing normative OpenSpec requirements.
2. Prepare the retrospective and human-acceptance evidence, including verified
   scenarios, unresolved discrepancies/waivers, deletion/export boundaries,
   and exact playback versus simulation distinction.

**TDD exception:** document-only reconciliation has no meaningful failing
behavioral test. Verify it with `python3 scripts/check_docs.py`, internal
Markdown link checking, strict OpenSpec validation, schema validation as
applicable, and complete diff review; do not claim results before running them.

**Review checkpoint:** final documentation/evidence review must confirm no
unrun claims, complete scope/non-goal coverage, append-only and secret-safe
deferred-boundary wording, replay claim precision, and no scope creep.

## Final verification intent

Only after the final review is clean, run and record (without presupposing
success):

- focused Rust tests for each runtime slice;
- `cargo fmt -- --check`;
- `cargo clippy --all-targets -- -D warnings`;
- `cargo test --all-targets`;
- `python3 scripts/check_docs.py` and repository internal Markdown/link checks;
- `openspec validate --all --strict --no-interactive`;
- schema validation for persisted/runtime artifacts, as applicable;
- `git diff --check`;
- clean complete diff review, including intended and untracked files.

## Intended commit grouping

Do not create commits during plan generation. Once tests are green and the
corresponding review loop is clean, group changes logically:

1. storage foundation, migrations, and artifact lineage;
2. append-only event, lifecycle, projection, and replay behavior;
3. reconciliation and guarded recovery;
4. narrowed documentation, retrospective, and evidence.

No documentation or evidence commit precedes green tests for the behavior it
describes.
