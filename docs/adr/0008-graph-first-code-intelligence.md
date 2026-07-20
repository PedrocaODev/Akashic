# ADR 0008: Graph-first, source-authoritative code intelligence

**Status:** Accepted

Graphify begins as a code-only adapter with a soft graph-first navigation model while source files remain authoritative. LSP is optional. Graph output cannot silently rewrite source truth or become a required runtime dependency.
