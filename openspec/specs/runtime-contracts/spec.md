# runtime-contracts Specification

## Purpose
Defines the bootstrap daemon's secure runtime, protocol, identity, and
shutdown contracts.
## Requirements
### Requirement: Socket path and permissions
The daemon MUST use `${XDG_RUNTIME_DIR}/akashic` only when the XDG runtime path is absolute, its ancestors are non-symlinked and owned by root or the current UID and not group/other writable, and the selected runtime base and its `akashic` child are current-UID-owned, non-symlinked, and not group/other accessible, otherwise it MUST use `${XDG_STATE_HOME:-$HOME/.local/state}/akashic/run` with the same selected-base and child checks; ordinary root-owned ancestors such as `/`, `/run`, and `/home` MUST be allowed when they are not group/other writable; names MUST be `daemon.sock` and `daemon.lock`; the Akashic directory MUST be mode 0700, lock and socket MUST be mode 0600, creation MUST use umask 077, and symlinks or non-owned components MUST be rejected.

#### Scenario: Runtime directory selection
- **WHEN** a safe absolute XDG runtime directory exists
- **THEN** the daemon MUST use its `akashic` child; otherwise it MUST use the stated state fallback

#### Scenario: Unsafe path
- **WHEN** a trusted ancestor is symlinked, owned by neither root nor the current UID, or group/other writable, or when the selected per-user base or Akashic child is symlinked, not current-UID-owned, or group/other accessible
- **THEN** startup MUST fail closed without binding or unlinking a socket

### Requirement: Lock and socket race safety
The daemon MUST acquire an exclusive nonblocking lock on `daemon.lock` before inspecting, removing, or binding `daemon.sock`; if the lock is unavailable it MUST report `lifecycle.daemon_running` and touch nothing; while holding the lock, an absent socket MAY be bound, an owned-by-other-UID socket, symlink, or non-socket MUST fail closed, a current-user socket MUST be handshaken, a responsive socket MUST cause conflict without unlinking, and only connection-refused/no-listener evidence followed by unchanged inode/device recheck under the held lock MAY permit unlink and bind; shutdown cleanup MUST occur only while holding the lock and only when inode/device still match the socket created by that daemon.

#### Scenario: Active singleton
- **WHEN** another daemon holds the lock or a current-user socket responds
- **THEN** startup MUST return `lifecycle.daemon_running` and MUST NOT unlink or modify the active socket

#### Scenario: Verified stale socket
- **WHEN** the lock is held, the socket is current-user owned, connection is refused with no listener, and inode/device are unchanged
- **THEN** startup MAY unlink that socket and bind a new one

### Requirement: Peer authorization
The daemon MUST authorize the peer UID using Linux `SO_PEERCRED` or an equivalent supported primitive before handshake parsing or request processing, and an unauthorized peer MUST receive or cause `authorization.peer_uid` without an effect.

#### Scenario: Unauthorized peer
- **WHEN** the connecting UID differs from the daemon UID
- **THEN** the daemon MUST reject the connection before handshake processing

### Requirement: Protocol sequencing and response identity
After peer UID authorization, the first valid frame MUST be `handshake.request`; `health.request` before a successful handshake MUST return `protocol.malformed` without an effect; every response and error MUST receive a new UUID `id`; every response and error `correlation_id` MUST equal the triggering request `id`; and the following canonical bootstrap objects are valid illustrative examples whose UUID and semver placeholder values vary at runtime while their field names, types, kinds, payload shape, and correlation relationships are normative: `{"protocol":"akashic.local","version":1,"kind":"handshake.request","id":"11111111-1111-4111-8111-111111111111","payload":{"client_role":"jsonl","client_version":"0.1.0","client_instance_id":"22222222-2222-4222-8222-222222222222"}}`, `{"protocol":"akashic.local","version":1,"kind":"handshake.response","id":"33333333-3333-4333-8333-333333333333","correlation_id":"11111111-1111-4111-8111-111111111111","payload":{"daemon_version":"0.1.0","daemon_instance_id":"44444444-4444-4444-8444-444444444444","protocol_version":1,"capabilities":["health"]}}`, `{"protocol":"akashic.local","version":1,"kind":"health.request","id":"55555555-5555-4555-8555-555555555555","payload":{}}`, `{"protocol":"akashic.local","version":1,"kind":"health.response","id":"66666666-6666-4666-8666-666666666666","correlation_id":"55555555-5555-4555-8555-555555555555","payload":{"status":"ok","daemon_version":"0.1.0","daemon_instance_id":"44444444-4444-4444-8444-444444444444","protocol_version":1}}`, and `{"protocol":"akashic.local","version":1,"kind":"error.response","id":"77777777-7777-4777-8777-777777777777","correlation_id":"55555555-5555-4555-8555-555555555555","error":{"code":"protocol.malformed","message":"malformed request","retryable":false}}`.

#### Scenario: Sequencing and correlation
- **WHEN** an authorized client sends health before handshake or sends a valid handshake/health request
- **THEN** the daemon MUST return `protocol.malformed` for the first case without effect and MUST return a new response UUID correlated to each triggering request for the valid case

### Requirement: Protocol constants and handshake
The protocol identifier MUST be `akashic.local` and the protocol version MUST be unsigned integer 1; envelope fields MUST be `protocol` string, `version` unsigned integer, `kind` string, `id` UUID string, optional `correlation_id` UUID string, and exactly one of `payload` object or `error` object; request kinds MUST be exactly `handshake.request` and `health.request`, response kinds MUST be exactly `handshake.response`, `health.response`, and `error.response`; handshake payload MUST contain `client_role` enum `tui|jsonl|doctor`, `client_version` semver string, and `client_instance_id` UUID string; a compatible handshake response payload MUST contain `daemon_version` semver string, `daemon_instance_id` UUID string, `protocol_version` unsigned integer 1, and `capabilities` array containing exactly `health`; incompatible or incomplete handshakes MUST return `protocol.unsupported_version` or `protocol.malformed` without an effect.

#### Scenario: Compatible handshake
- **WHEN** a client sends a complete supported handshake request
- **THEN** the daemon MUST return the exact versioned handshake response fields and capability

#### Scenario: Invalid handshake
- **WHEN** a handshake has wrong constants, types, role, version, or required fields
- **THEN** the daemon MUST return the applicable structured protocol error and execute no effect

### Requirement: JSONL framing and health
JSONL MUST be UTF-8 with exactly one JSON object per newline-delimited line, each line including its newline MUST be at most 1048576 bytes by default and configurable only within the stated range, malformed, invalid-UTF-8, incomplete, or oversized input MUST return `protocol.malformed` or `protocol.oversized` without a partial-frame effect, `health.request` payload MUST be `{}`, `health.response` payload MUST contain `status` equal to `ok`, daemon version, daemon instance ID, and protocol version 1, repeated health requests MUST be idempotent, and JSONL protocol records MUST go only to stdout while diagnostics go only to stderr.

#### Scenario: Health request
- **WHEN** a valid `health.request` with `{}` is received
- **THEN** the process MUST return the exact idempotent health response payload

#### Scenario: Framing violation
- **WHEN** a line exceeds 1048576 bytes including newline or is malformed, invalid UTF-8, or incomplete
- **THEN** the process MUST return the applicable error response and MUST execute no effect
