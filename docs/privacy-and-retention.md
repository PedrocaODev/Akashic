# Privacy and retention

Raw task history is retained locally indefinitely until explicit deletion. OS filesystem protection is the initial boundary; it is not a promise that pre-persistence redaction can detect every secret. Storage warnings explain exposure and size without automatic purge.

Exports are explicit, scoped, and warn that task history may contain private source or credentials. Deletion follows artifact and evidence lineage: usable descendants are removed or invalidated while the durable deletion record explains what was requested. Exact retention and deletion behavior belongs in OpenSpec.

Telemetry is opt-in, minimized, and separate from local history. A public release is blocked until a private vulnerability-reporting channel is available. Do not disclose secrets or private task data in public reports.
