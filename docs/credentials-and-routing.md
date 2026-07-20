# Credentials and routing

Admissible credentials are official OpenAI, Anthropic, Gemini, OpenRouter, or OpenAI-compatible local API/service/workload credentials. CLI subscription tokens are never imported. Resolution is daemon-controlled: approved explicit task reference, configured project/provider reference, OS keyring, then an explicitly configured headless secret source; absence is a visible error.

Credentials are accessed only by the daemon, never injected into prompts, child worktrees, logs, or unapproved processes. Redaction covers events, evidence, diagnostics, and exports. Rotation replaces references without rewriting history; deletion removes usable material while preserving non-secret lineage and records the deletion operation.

The provider allowlist and capability model govern model, tool, network, cost, and data permissions. Fallback is only to an approved provider with compatible capabilities and explicit evidence; it is not an implicit credential search. Routing starts static and transparent. Adaptive proposals require shadow evaluation, approval, activation, and rollback.
