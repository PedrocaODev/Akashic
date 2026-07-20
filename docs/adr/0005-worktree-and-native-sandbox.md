# ADR 0005: Worktrees and native sandbox

**Status:** Accepted

## Decision

Every task uses a task integration worktree. Logical child writer worktrees are sibling directories, and the daemon owns Git integration and ephemeral commits. Native Linux execution uses bubblewrap, Landlock, seccomp, private task home/environment, and limits. Docker, network, display, SSH, and D-Bus are denied by default; security levels are explicit with no silent fallback.

## Consequences

Writers remain isolated and integration is auditable. The host must meet native sandbox prerequisites; inability to meet a requested security level blocks execution rather than weakening it.
