# Development

The bootstrap Rust package is runnable, but it is not an installation or product-release workflow. The [implementation plan](implementation-plan.md) and [qualification matrix](qualification-matrix.md) define the through-v1 evidence path; normative bootstrap behavior remains in [OpenSpec](../openspec/specs/).

Start with the relevant accepted decision, especially [ADR 0010](adr/0010-modular-monolith-and-dependency-direction.md), and OpenSpec change. Keep authored explanations in Markdown and keep normative requirements in OpenSpec. Do not edit `openspec/**` without ownership transfer.

Future implementation work must test deterministic transitions, approval boundaries, sandbox denial, credential handling, worktree ownership, append-only evidence, replay semantics, and distinct terminal outcomes. Risk-based verification must produce evidence, followed by a full retrospective for every task.

## Bootstrap checks

```text
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
python3 scripts/check_docs.py
openspec validate --all --strict --no-interactive
```

The secure runtime and peer authorization are Linux-specific. WSL2 is qualified
by the automated Linux-path tests available in this repository, but the
unauthorized-peer test is conditionally skipped when the runner cannot perform
UID switching with `setpriv`. Non-Linux targets are explicitly unsupported by
the bootstrap rather than silently downgraded.

Documentation verification consists of checking relative Markdown links and reviewing the complete diff. Future platform qualification should separately report Linux, WSL2, headless Linux, and the explicitly unverified Apple targets; Rust, Java, and Kotlin Multiplatform are reference ecosystems.

## Source-boundary workflow

- Keep one active writer per touched module; overlapping edits require explicit handoff or serialization, and the affected module owner/specialist reviews cross-module changes. Separate mechanical moves from behavioral changes.
- `schema.rs` owns DDL, migrations, and physical schema validation. `store.rs` owns connection lifecycle and transactions and invokes schema operations; capability-owned transactional statements may remain with their capability, but no direct schema mutation occurs outside `schema.rs`. Schema/migration changes require schema review and physical-compatibility evidence.
- Do not add `#[path]` source inclusion. Executable-level external contract tests use the binary; crate-private capability behavior uses unit tests in or under its owning module. A public Rust API requires approval.
- Route recovery or filesystem-identity changes through security review, including recovery/security review of identity and boundary changes. Do not add broad `dead_code` allowances or arbitrary LOC limits.
- Obtain approval before adding dependencies, changing schema version or semantics, exposing public APIs, changing security or destructive boundaries, or reversing dependency direction.
