# ADR 0007: Defer public extension APIs

**Status:** Accepted

## Decision

Support Agent Skills compatibility, MCP, and declarative customization as integration directions, but provide no public executable plugin ABI in v1.

## Consequences

The initial security and compatibility surface stays smaller. Extension needs must be validated through bounded declarative or protocol-based work before a public executable ABI is reconsidered.
