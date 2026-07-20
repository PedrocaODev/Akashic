# Qualification matrix

| Area | Linux | WSL2 | Headless Linux | Evidence |
|---|---|---|---|---|
| Harness/runtime | target | target | target | build, lifecycle, recovery |
| Sandbox | native profile | kernel/AppArmor diagnostics | native profile | capability and denial tests |
| Clients | TUI/JSONL | TUI/JSONL | JSONL | protocol and crash tests |
| Providers | allowlisted targets | same, subject to network policy | same, configured headless credentials | credential/redaction tests |
| Storage/replay | SQLite/filesystem | SQLite/filesystem | SQLite/filesystem | migration, projection, playback tests |
| Evaluation | risk-based | WSL-specific | non-display | evidence and retrospective |

Rust is the implementation target; Java and Kotlin Multiplatform are reference ecosystems for projects Akashic may assist. Apple targets are explicitly unverified from Linux and are not a public-v1 qualification claim. Qualification must report unsupported capabilities rather than silently downgrade.
