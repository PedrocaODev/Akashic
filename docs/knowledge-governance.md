# Knowledge governance

Knowledge is scoped to task, project, or global context. Each scope is bounded by injection budget, item count and size, provenance, and expiry/review requirements. FTS supports retrieval; it does not make memory authoritative.

Learning follows proposal → evaluation → approval → activation → rollback. Deletion records lineage so derived entries can be found and removed or invalidated. Concrete numeric defaults are intentionally locked by the later OpenSpec change, not silently omitted here.

Agent Skills compatibility, MCP, and declarative customization are supported directions without a public executable plugin ABI in v1. Router changes are shadow-evaluated before any approved activation. Graphify remains a code-only, source-authoritative adapter; adaptive knowledge must not rewrite source truth.
