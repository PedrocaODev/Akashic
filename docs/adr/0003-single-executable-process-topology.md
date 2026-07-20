# ADR 0003: Single executable, process topology

**Status:** Accepted

## Decision

Akashic is distributed as one harness executable with separate daemon, TUI, and JSONL processes. The daemon is the control-plane authority; clients communicate through its supported boundary.

## Consequences

The topology supports interactive, automation, and headless use without duplicating authority. Process protocol details are deferred to the relevant OpenSpec change.
