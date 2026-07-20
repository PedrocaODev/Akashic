# Development

Akashic has no implementation or installation workflow yet. This page describes the intended contribution boundary, not runnable commands. The [implementation plan](implementation-plan.md) and [qualification matrix](qualification-matrix.md) define the through-v1 evidence path.

Start with the relevant accepted decision and OpenSpec change. Keep authored explanations in Markdown and keep normative requirements in OpenSpec. Do not edit `openspec/**` without ownership transfer.

Future implementation work must test deterministic transitions, approval boundaries, sandbox denial, credential handling, worktree ownership, append-only evidence, replay semantics, and distinct terminal outcomes. Risk-based verification must produce evidence, followed by a full retrospective for every task.

Documentation verification consists of checking relative Markdown links and reviewing the complete diff. Future platform qualification should separately report Linux, WSL2, headless Linux, and the explicitly unverified Apple targets; Rust, Java, and Kotlin Multiplatform are reference ecosystems.
