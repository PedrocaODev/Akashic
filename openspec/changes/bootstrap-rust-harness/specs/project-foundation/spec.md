## ADDED Requirements

### Requirement: Command contracts
The executable MUST support exactly `akashic daemon`, `akashic tui`, `akashic run --jsonl`, `akashic doctor`, and `akashic version`; CLI parsing MUST occur before mode entry, and invalid syntax that does not successfully select `doctor` MUST emit generic `usage.invalid` JSON on stderr, empty stdout, and exit 2; `daemon` MUST start one daemon and block until bounded shutdown; `run --jsonl` MUST connect to the daemon and accept only bootstrap handshake/health JSONL on stdin while writing protocol records to stdout and diagnostics to stderr; once `akashic doctor` mode is successfully selected, doctor MUST wrap all subsequent config, runtime-directory, socket, daemon availability, handshake, and health outcomes in exactly one stdout JSON object with `status` enum `ok|degraded|error`, `version` semver string, `protocol` object `{ "identifier": "akashic.local", "version": 1 }`, and `checks` array objects containing `name` string, `status` enum `ok|warning|error`, nullable stable `code`, and redacted `message` string, MUST NOT emit the generic CLI error object for those outcomes, MUST return overall `degraded` with a `daemon` check code `lifecycle.daemon_unavailable` and exit 4 when no socket/listener exists but local prerequisites are safe, MUST return overall `ok` and exit 0 after successful checks and health, and MUST return overall `error` and exit 1 for invalid config/path/socket/handshake; `version` MUST print exactly `akashic <semver>` plus newline and exit zero without daemon access; `tui` MUST connect, handshake, display only a minimal daemon health/version screen, support quit/cancel, and not claim task functionality; unknown or conflicting usage MUST produce a structured failure and nonzero exit.

#### Scenario: Version command
- **WHEN** `akashic version` is run
- **THEN** stdout MUST contain exactly one `akashic <semver>` line, the process MUST exit zero, and no daemon access MUST occur

#### Scenario: Invalid usage
- **WHEN** an unknown command, option, or conflicting mode is supplied
- **THEN** the process MUST return a `usage.invalid` structured failure and exit nonzero

#### Scenario: Invalid syntax before doctor selection
- **WHEN** CLI syntax is invalid and `akashic doctor` mode is not successfully selected
- **THEN** stderr MUST contain generic `usage.invalid` JSON, stdout MUST be empty, and the process MUST exit 2

#### Scenario: Invalid config inside doctor
- **WHEN** `akashic doctor` is successfully selected and config loading fails
- **THEN** stdout MUST contain exactly one doctor result with overall `error`, a check carrying `config.invalid` or the applicable config code, and no generic CLI error object

#### Scenario: No daemon inside doctor
- **WHEN** `akashic doctor` is successfully selected, local prerequisites are safe, and no socket/listener exists
- **THEN** stdout MUST contain exactly one doctor result with overall `degraded`, a `daemon` check carrying `lifecycle.daemon_unavailable`, and the process MUST exit 4

#### Scenario: Handshake failure inside doctor
- **WHEN** `akashic doctor` is successfully selected and a safe socket fails handshake or health validation
- **THEN** stdout MUST contain exactly one doctor result with overall `error` and a check carrying the underlying protocol or authorization code, and the process MUST exit 1

### Requirement: Configuration schema and precedence
Configuration MUST use built-in defaults below user config, project config, `AKASHIC_*` environment overrides, and global CLI overrides in that order; user config MUST be `${XDG_CONFIG_HOME:-$HOME/.config}/akashic/config.toml`; project config MUST be `.akashic/config.toml` in the nearest Git root from the current directory or in the current directory when no Git root exists; both files MUST require `config_version = 1`; supported keys and defaults MUST be `log_level = "info"`, `shutdown_timeout_seconds = 10`, and `jsonl_max_line_bytes = 1048576`; supported environment names MUST be `AKASHIC_LOG_LEVEL`, `AKASHIC_SHUTDOWN_TIMEOUT_SECONDS`, and `AKASHIC_JSONL_MAX_LINE_BYTES`; supported global CLI flags before the command MUST be `--log-level`, `--shutdown-timeout-seconds`, and `--jsonl-max-line-bytes`; log level MUST be exactly one of `error|warn|info|debug|trace`, timeout MUST be an integer from 1 through 600, line size MUST be an integer from 1024 through 1048576, unknown keys MUST fail closed, raw secret fields MUST be rejected, and future credentials MUST use references rather than values.

#### Scenario: Precedence
- **WHEN** a setting appears at several supported layers
- **THEN** the CLI value MUST override environment, project, user, and built-in values in that order

#### Scenario: Invalid configuration
- **WHEN** a file omits `config_version`, declares a version other than 1, contains an unknown key, secret-like key, invalid type, or out-of-range value
- **THEN** loading MUST fail closed with `config.invalid`, `config.unsupported_version`, or `config.secret_forbidden` as applicable

### Requirement: Error and redaction schema
Every bootstrap error object MUST contain `code` as a stable string, `message` as a redacted string, and `retryable` as a boolean, and MAY contain `correlation_id` as a UUID string; bootstrap MUST support at minimum `usage.invalid`, `config.invalid`, `config.unsupported_version`, `config.secret_forbidden`, `protocol.malformed`, `protocol.unsupported_version`, `protocol.oversized`, `authorization.peer_uid`, `lifecycle.daemon_running`, `lifecycle.daemon_unavailable`, `lifecycle.shutdown_timeout`, and `internal.unexpected`; `retryable` MUST be false for `usage.invalid`, every `config.*`, `protocol.malformed`, `protocol.unsupported_version`, `protocol.oversized`, and `authorization.peer_uid`, and MUST be true for `lifecycle.daemon_running`, `lifecycle.daemon_unavailable`, `lifecycle.shutdown_timeout`, and `internal.unexpected`; secret-like names are case-insensitive names containing `token`, `secret`, `password`, `api_key`, `authorization`, `cookie`, or `credential`, and exact credential-broker-resolved values are secret-like; logs MUST redact these values before serialization and MUST NOT log full environment or config contents.

#### Scenario: Redacted error
- **WHEN** invalid input or a secret-bearing diagnostic produces an error
- **THEN** the response MUST use the applicable stable code and MUST contain no raw secret value

### Requirement: Error destinations and exits
Non-JSONL CLI or startup errors before successful mode selection, excluding post-selection doctor results, MUST emit exactly one JSON error object on process stderr and nothing on process stdout; after valid framing, `akashic run --jsonl` request-level protocol errors MUST use `error.response` on that process stdout; after peer authentication, daemon socket request errors MUST use `error.response` over that client's Unix socket connection and MUST NOT use daemon process stdout; post-selection doctor config, runtime, socket, availability, handshake, and health outcomes MUST use exactly one doctor result on stdout instead of the generic error object; diagnostics MUST remain on stderr; CLI or startup exit codes MUST be 2 for usage/configuration, 3 for protocol/authorization, 4 for `lifecycle.daemon_running`, 124 for `lifecycle.shutdown_timeout`, and 1 for internal errors; request-level JSONL errors MUST NOT terminate a healthy stream unless framing continuation is unsafe.

#### Scenario: CLI error destination
- **WHEN** a non-JSONL CLI command fails validation
- **THEN** stderr MUST contain exactly one error object, stdout MUST be empty, and the mapped exit code MUST be returned

#### Scenario: JSONL request error
- **WHEN** a valid frame requests an unsupported operation
- **THEN** stdout MUST receive one `error.response`, diagnostics MUST remain on stderr, and the stream MUST continue

#### Scenario: Daemon socket error destination
- **WHEN** an authenticated daemon client sends a request that produces a protocol error
- **THEN** the client socket MUST receive one `error.response`, daemon stdout MUST remain unused, and diagnostics MUST remain on stderr

### Requirement: Signals and shutdown
SIGINT and SIGTERM MUST initiate graceful shutdown, any second SIGINT after graceful shutdown begins MUST immediately terminate with exit 130, any second SIGTERM after graceful shutdown begins MUST immediately terminate with exit 143 regardless of the first signal, shutdown MUST stop accepting requests, close the listener, cancel bootstrap-owned children, wait no longer than the configured timeout default of 10 seconds, and then emit `lifecycle.shutdown_timeout` exactly once to stderr and exit 124; clean requested shutdown MUST exit zero, and bootstrap MUST NOT use future task terminal outcome taxonomy.

#### Scenario: Graceful signal
- **WHEN** the first SIGINT or SIGTERM is received and children finish within the timeout
- **THEN** the listener MUST close, children MUST be cancelled, and the process MUST exit zero

#### Scenario: Timed-out signal
- **WHEN** children do not finish before the configured timeout
- **THEN** the process MUST force termination and return `lifecycle.shutdown_timeout` with nonzero status
