# Qualification matrix

| Area | Bootstrap status | Public-v1 status | Linux | WSL2 | Headless Linux | Evidence |
|---|---|---|---|---|---|---|
| Harness/runtime | qualified health-only baseline | not qualified | automated Linux | automated WSL2-compatible path; UID-switch qualification noted | target | cargo tests, lifecycle, secure runtime, protocol checks |
| Sandbox | boundary checks only; task sandbox future | not qualified | native profile | kernel/AppArmor diagnostics | native profile | capability and denial tests |
| Clients | health-only daemon/TUI/JSONL paths | future product-capable clients; not qualified | health-only paths | health-only paths | health-only JSONL path | protocol and crash tests when applicable |
| Providers | not implemented | not qualified | future | future | future | credential/redaction tests |
| Storage/replay | baseline filesystem only | not qualified | SQLite/filesystem | SQLite/filesystem | SQLite/filesystem | migration, projection, playback tests |
| Evaluation | risk-based health checks | not qualified | risk-based | WSL-specific | non-display | evidence and retrospective |

Rust is the implementation target; Java and Kotlin Multiplatform are reference ecosystems for projects Akashic may assist. The bootstrap secure runtime is explicitly Linux-only; Apple and other non-Linux targets are unverified and are not a public-v1 qualification claim. WSL2 uses the Linux implementation; UID-switch-dependent peer rejection may require manual qualification when `setpriv` is unavailable. Qualification must report unsupported capabilities rather than silently downgrade. Bootstrap status records the health-only baseline; public-v1 status requires the later product-capable milestones and their evidence, including successful cross-UID peer rejection requalified on a Linux environment capable of UID switching. The current waiver remains visible and no success claim is made until that requalification.
