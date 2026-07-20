# Qualification matrix

| Area | Linux | WSL2 | Headless Linux | Evidence |
|---|---|---|---|---|
| Harness/runtime | automated Linux | automated WSL2-compatible path; UID-switch qualification noted | target | cargo tests, lifecycle, secure runtime, protocol checks |
| Sandbox | native profile | kernel/AppArmor diagnostics | native profile | capability and denial tests |
| Clients | TUI/JSONL | TUI/JSONL | JSONL | protocol and crash tests |
| Providers | allowlisted targets | same, subject to network policy | same, configured headless credentials | credential/redaction tests |
| Storage/replay | SQLite/filesystem | SQLite/filesystem | SQLite/filesystem | migration, projection, playback tests |
| Evaluation | risk-based | WSL-specific | non-display | evidence and retrospective |

Rust is the implementation target; Java and Kotlin Multiplatform are reference ecosystems for projects Akashic may assist. The bootstrap secure runtime is explicitly Linux-only; Apple and other non-Linux targets are unverified and are not a public-v1 qualification claim. WSL2 uses the Linux implementation; UID-switch-dependent peer rejection may require manual qualification when `setpriv` is unavailable. Qualification must report unsupported capabilities rather than silently downgrade.
