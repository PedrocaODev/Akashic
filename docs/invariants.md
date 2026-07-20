# Invariants

These invariants describe the intended control plane; enforceable normative forms belong in OpenSpec. See the [lifecycle](lifecycle-and-outcomes.md), [artifacts](artifacts-and-replay.md), [sandboxing](sandboxing.md), and [privacy](privacy-and-retention.md) decisions.

- LLM output is untrusted strategy, never authoritative state or policy.
- State transitions, approvals, scheduling, evidence, and invariants are deterministic and recorded.
- Markdown is the canonical authored artifact; SQLite records are append-only and do not silently rewrite authored history.
- Each task has one task integration worktree. Logical child writer worktrees are sibling directories, and only the daemon integrates them.
- Security level and granted capabilities are explicit. Failure to achieve a level is visible; no silent fallback.
- Network, display, SSH, D-Bus, and Docker are denied by default.
- Provider credentials are official API/service/workload credentials; CLI subscription tokens are never imported.
- Exact event playback cannot be represented as captured-outcome simulation.
- Terminal outcomes remain distinct: `verified`, `accepted_with_waivers`, `accepted_partial`, `blocked`, `aborted`, `failed`.
- Learning cannot activate without proposal, evaluation, approval, and rollback capability.
- Raw task history is local until explicit deletion; telemetry is opt-in.
