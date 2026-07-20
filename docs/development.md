# Development

The bootstrap Rust package is runnable, but it is not an installation or product-release workflow. The [implementation plan](implementation-plan.md) and [qualification matrix](qualification-matrix.md) define the through-v1 evidence path; normative bootstrap behavior remains in [OpenSpec](../openspec/changes/bootstrap-rust-harness/).

Start with the relevant accepted decision and OpenSpec change. Keep authored explanations in Markdown and keep normative requirements in OpenSpec. Do not edit `openspec/**` without ownership transfer.

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
