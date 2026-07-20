# Akashic

**Bootstrap implementation / research status.** The initial Rust harness bootstrap is runnable for its daemon, TUI, JSONL, doctor, and version contracts; dependent task, provider, sandbox, and storage systems remain unimplemented. This repository records the accepted product, architecture, security, and delivery baseline for an open-source, local-first Rust AI coding harness.

Akashic is intended for Linux, WSL2, and headless Linux. One harness executable will provide separate daemon, TUI, and JSONL client processes. An LLM orchestrator may propose strategy, but a deterministic Rust runtime owns state, policy, approvals, evidence, scheduling, and invariants.

## Lifecycle

Tasks are expected to move through preflight, Task Contract, conditional research, plan, approval, implementation, review/fix, risk-based verification, mandatory retrospective, human acceptance, and explicit delivery. See [lifecycle and outcomes](docs/lifecycle-and-outcomes.md), [artifacts and replay](docs/artifacts-and-replay.md), and the [implementation plan](docs/implementation-plan.md).

## Status and scope

Normative behavior will be specified in `openspec/`, not inferred from this overview. The current roadmap is [docs/roadmap.md](docs/roadmap.md); design decisions are recorded in [ADRs](docs/adr/0001-deterministic-control-plane.md).

The bootstrap can be checked with `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets`. It is a development harness, not a packaged product; see [development checks](docs/development.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Akashic is released under the [Apache License 2.0](LICENSE).
