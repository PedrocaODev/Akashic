# ADR 0004: Provider normalization and routing

**Status:** Accepted

## Decision

Target OpenAI, Anthropic, Gemini, OpenRouter, and OpenAI-compatible local providers behind normalized runtime policy. Accept only official API, service, or workload credentials; never import CLI subscription tokens. Use a static, transparent router first.

## Consequences

Provider-specific behavior is isolated without granting providers control-plane authority. Adaptive routing is deferred until it can be proposed, evaluated, approved, and rolled back.
