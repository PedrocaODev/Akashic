# Privacy and retention

Raw task history is retained locally indefinitely until explicit deletion. OS filesystem protection is the initial boundary; it is not a promise that pre-persistence redaction can detect every secret. Storage warnings explain exposure and size without automatic purge.

Export and deletion are not implemented by this change. Scoped export,
retention policy, deletion/descendant invalidation, and secret-safe deletion
lineage require future OpenSpec work. The broader baseline remains: any future
export must be explicit and warn that history may contain private source or
credentials, and deletion must follow artifact and evidence lineage.

Telemetry is opt-in, minimized, and separate from local history. A public release is blocked until a private vulnerability-reporting channel is available. Do not disclose secrets or private task data in public reports.
