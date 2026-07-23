# Slice 1 security boundary

## Verified evidence

Current reproducible results: Slice 1/2 focused 52 passed; Slice 3 30 passed;
Slice 3 migration 10 passed; full suite 171 passed;
security 14 passed; checkpoint 22 passed; CLI 9 passed. Historical RED
chronology is unavailable and is not independently verified.

Proven by these tests: current operation results are restricted to accepted or
rejected; accepted rows require exactly one matching version and lineage row,
while rejected rows require no accepted version/lineage and a linked
discrepancy; malformed values and orphan rejected rows fail closed. Accepted
current versions require matching operation and lineage rows with agreeing
result, parent/version, artifact, owner, source, and ancestry values; legacy
migration checks the reviewed columns/types, operation
uniqueness, foreign-key absence, relational validity, and content hash before
migration; transactional source lineage and structured
expected/observed owner/source/ancestry discrepancy values remain covered.

Daemon protocol and custom VFS dispositions remain unchanged. Exact hostile-race
proof remains unresolved and is not claimed.

## VFS limitation

Ordinary `rusqlite` APIs do not expose stable main/WAL/SHM descriptor proof
across the complete SQLite lifecycle. Slice 1 does not build a custom VFS or
claim a race-free SQLite descriptor proof.
