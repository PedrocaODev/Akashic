# Akashic

Akashic is an Apache-2.0, local-first Rust coding harness for Linux, WSL2,
and headless Linux. It ships one executable with separate daemon, TUI, and
JSONL process modes.

## Boundaries and constraints

- Deterministic Rust runtime owns state transitions, policy, approvals,
  evidence, effects, scheduling, and replay.
- LLMs propose strategy through typed proposals only.
- Authored Markdown files are canonical artifacts. Runtime events, findings,
  and evidence are structured SQLite records.
- Approval binds the artifact hash, base commit, policy, and capabilities.
- Native execution uses Git worktrees with bwrap/Landlock/seccomp, private
  environment, and fail-closed required protections.
- Providers, agents, substantive TUI behavior, Graphify, learning, and
  Headroom are dependent future phases, not bootstrap implementation.

Normative behavior belongs in `openspec/` specs. Root documentation and
`docs/` explain intent and decisions; they are not normative substitutes.

## Change conventions

Change IDs are kebab-case. Dependencies are declared in proposal/design text
as `depends-on: <id>` and must be completed or archived before dependent work.
Do not create speculative change directories.

Every implementation change uses test-first slices, review/fix rerun,
`verify.md` evidence, and a `retrospective.md`. Evidence must be real and
reproducible; pending documents must never claim completed verification.

## Roadmap

1. `bootstrap-rust-harness`
2. `build-artifact-runtime` (depends 1)
3. `add-provider-runtime` (depends 2)
4. `secure-worktree-execution` (depends 2)
5. `implement-agent-convergence` (depends 2,3,4)
6. `expose-daemon-and-clients` (depends 5)
7. `add-declarative-tools` (depends 5,6)
8. `integrate-code-intelligence` (depends 4,5)
9. `add-parallel-worktree-execution` (depends 4,5)
10. `evaluate-context-optimization` (depends 2,3,5)
11. `add-adaptive-knowledge` (depends 3,5,6,7)
12. `qualify-public-v1` (depends on all completed/archived predecessors;
    Headroom experiment may conclude no adoption)

## Non-goals

Bootstrap does not include providers, SQLite artifact runtime, production
sandboxing, worktrees, agent lifecycle, substantive TUI, Graphify/LSP,
memory/skills, Headroom, or public extension APIs.
