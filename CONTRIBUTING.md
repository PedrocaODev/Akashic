# Contributing

Akashic has an implemented bootstrap baseline but remains in staged
development. Contributions should clarify accepted behavior rather than
inventing unapproved APIs or claiming unfinished dependent features.

## Before changing files

Read `README.md`, the relevant document in `docs/`, and the applicable OpenSpec change/spec. Normative behavior belongs in OpenSpec; explanatory material should link to it. Do not edit `openspec/**` unless the owner explicitly transfers that work.

## Documentation

Use concise Markdown, stable terminology, and Mermaid only where a diagram explains a relationship. Mark assumptions and unverified platform behavior explicitly. Keep links relative and verify them before submitting.

## Future code contributions

Rust code must preserve the deterministic control-plane boundary, explicit security levels, append-only evidence, and task/worktree ownership described in the architecture and invariants documents. New providers, permissions, learning activation, and delivery behavior require an approved spec and tests.

## Review

Describe the problem, the relevant accepted decision or spec, verification performed, and any unresolved limitation. Do not include credentials, captured secrets, or raw private task data.

Changes go through pull requests targeting `main`. `@PedrocaODev` is the code owner and gatekeeper. Every pull request must describe its verification and any unresolved limitations.
