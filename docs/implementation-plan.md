# Implementation plan

**Status:** planning baseline; nothing here claims implementation. Normative behavior remains in `openspec/`.

## Boundaries and ownership

The daemon is the deterministic control plane: it owns task state, policy, approvals, scheduling, evidence, projections, worktree integration, and ephemeral commits. The orchestrator, planner, implementer, reviewer, and scoped fixers propose or perform bounded work; they do not own truth. TUI and JSONL are clients. Markdown owns authored intent and decisions; append-only SQLite owns runtime events, projections, findings, evidence, history, and replay. Current files and records have one explicit owner.

## Dependency stack choices

Rust is the implementation language and SQLite the local append-only store. Native Linux isolation uses bubblewrap, Landlock, and seccomp; Git worktrees provide task isolation. FTS supports bounded curated memory. Graphify is an optional code-only adapter; LSP is optional. Provider adapters target OpenAI, Anthropic, Gemini, OpenRouter, and OpenAI-compatible local endpoints. Agent Skills and MCP are integration formats, not executable v1 plugin ABI dependencies. ai-jail is GPL reference material only, never a dependency. Concrete crate, schema, and numeric policy choices require their OpenSpec changes.

## Through-v1 changes

| Change | Depends on | Scope / non-goal | Exit gate and documentation |
|---|---|---|---|
| `bootstrap-rust-harness` | none | workspace, executable skeleton, no product behavior | reproducible build checks; architecture/development docs |
| `build-artifact-runtime` | bootstrap-rust-harness | state, Markdown/SQLite boundary, events; no providers | replay and ownership tests; artifacts/replay docs |
| `add-provider-runtime` | build-artifact-runtime | normalized official-provider access; no arbitrary credential import | credential and routing evidence; credentials doc |
| `secure-worktree-execution` | build-artifact-runtime | task integration worktree and native profiles; no silent fallback | denial/escape tests; sandbox doc |
| `implement-agent-convergence` | build-artifact-runtime + add-provider-runtime + secure-worktree-execution | fixed core lifecycle; no dynamic unrestricted agents | lifecycle/reviewer evidence; lifecycle doc |
| `expose-daemon-and-clients` | implement-agent-convergence | daemon, TUI, JSONL boundaries; no alternate authority | protocol and crash recovery checks; architecture doc |
| `add-declarative-tools` | implement-agent-convergence + expose-daemon-and-clients | Agent Skills/MCP/declarative tools; no executable plugin ABI | safe loading tests; ADR 0007 |
| `integrate-code-intelligence` | secure-worktree-execution + implement-agent-convergence | Graphify code adapter, optional LSP; no source replacement | source-authority evidence; ADR 0008 |
| `add-parallel-worktree-execution` | secure-worktree-execution + implement-agent-convergence | bounded sibling writers; no daemon bypass | integration/conflict tests; sandbox/architecture docs |
| `evaluate-context-optimization` | build-artifact-runtime + add-provider-runtime + implement-agent-convergence | evaluation-only Headroom lane; no port without evidence | comparative report and approval; ADR 0009 |
| `add-adaptive-knowledge` | add-provider-runtime + implement-agent-convergence + expose-daemon-and-clients + add-declarative-tools | governed bounded memory and router proposals; no silent learning | rollback/deletion lineage evidence; knowledge doc |
| `qualify-public-v1` | every completed predecessor; Headroom may conclude no adoption | release qualification; no promise for unverified Apple targets | matrix, security, privacy, and acceptance gates; qualification doc |

The direct dependency graph above is authoritative. Qualification waits for every completed predecessor, even if the Headroom experiment concludes no adoption.

## Verification and quality gates

Verification layers are unit/property checks, deterministic event/replay checks, integration tests, sandbox denial and capability checks, provider credential/redaction checks, crash/recovery checks, documentation/link checks, and platform qualification. Every task has risk-based verification, a full retrospective, then human acceptance. Global gates require formatting, linting, compilation, tests, dependency/license review, secret scanning, clean documentation links, and a reviewed diff. A release must also show no unresolved release-blocking private-reporting gap.

## Release progression

Research baseline → internal bootstrap → artifact-runtime milestone → supervised provider/sandbox milestones → daemon/client preview → bounded feature preview → public-v1 qualification. Each stage requires its exit evidence and human acceptance; pre-implementation documents are not release evidence.

## Known risks

Kernel and sandbox gaps, WSL policy differences, provider API drift, credential leakage, corrupted local stores, replay ambiguity, worktree conflicts, model-induced unsafe changes, unbounded memory, and insufficient Apple validation remain risks. The later specs must turn each into testable boundaries.
