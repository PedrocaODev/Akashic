# artifact-lineage Specification

## Purpose
TBD - created by archiving change build-artifact-runtime. Update Purpose after archive.
## Requirements
### Requirement: Daemon-owned artifact identity and explicit versioning
The daemon MUST own the runtime identity and ownership record for each authored Markdown artifact. A caller MUST use an explicit import/version operation to introduce or accept canonical Markdown; filesystem discovery, implicit scanning, or an unrequested rewrite MUST NOT create an accepted version. Each accepted version MUST record the artifact identity, exact content hash, schema version, source, author, ancestry, and the operation identity and lineage that accepted it.

#### Scenario: First import
- **WHEN** an explicit import supplies canonical Markdown with no existing artifact identity
- **THEN** the daemon MUST create one artifact identity and one immutable accepted version containing the supplied hash, schema version, source, author, ancestry, and operation lineage

### Requirement: Immutable hash-bound versions and idempotent operations
Accepted artifact versions MUST be immutable and MUST NOT be silently rewritten, replaced, or auto-merged. A repeated operation identity with the same content identity MUST return its original result without creating another version. A changed content identity MUST be handled as a new candidate/version operation and MUST NOT mutate the earlier version.

#### Scenario: Unchanged retry
- **WHEN** an import/version operation is retried with the same operation identity and content hash
- **THEN** the daemon MUST return the original result and MUST NOT append a duplicate accepted version or transition

#### Scenario: Changed content
- **WHEN** content for an existing artifact differs from its accepted content hash
- **THEN** the daemon MUST preserve the prior version and require a distinct version operation recording ancestry and the new hash

### Requirement: Fail-closed ownership and external-drift handling
The daemon MUST reject an import/version operation when ownership or lineage conflicts with an existing artifact, record enough non-secret conflict context to make the operation actionable, and require explicit resolution. It MUST NOT claim ownership, auto-merge, or rewrite content to resolve such a conflict. Before accepting a file-backed operation, it MUST compare the observed file identity and content hash with the expected lineage and MUST fail closed when external drift is detected.

#### Scenario: Ownership conflict
- **WHEN** an import targets an artifact owned by another actor or incompatible runtime identity
- **THEN** the operation MUST fail closed, preserve the existing artifact and versions, and return a resolvable conflict with recorded context

#### Scenario: External file drift
- **WHEN** a canonical Markdown file changes outside the daemon after its expected hash was recorded
- **THEN** the daemon MUST reject the operation as drift, MUST NOT overwrite the file or accept an unverified version, and MUST retain an actionable discrepancy

