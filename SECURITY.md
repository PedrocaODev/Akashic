# Security

Only the bootstrap runtime is implemented. Its documented Linux security
boundary is covered by its verification evidence; broader task sandboxing and
provider security remain future work.

The covered bootstrap boundary is the Linux executable health-check surface:
local process behavior, filesystem ownership/permissions, and the explicitly
denied capabilities exercised by the bootstrap checks. It does not claim a
task sandbox, provider isolation, or product-client security boundary. The
archived verification record is [here](openspec/changes/archive/2026-07-20-bootstrap-rust-harness/verify.md).

## Design baseline

The intended runtime uses bubblewrap, Landlock, seccomp, private task home/environment, and resource limits. Network, display, SSH, D-Bus, and Docker access are denied by default. Security levels are explicit and failure to obtain the requested level is an error, never a silent fallback.

Only official provider API, service, or workload credentials are accepted. Imported CLI subscription tokens are prohibited. Raw task history remains local until explicit deletion; telemetry is opt-in.

## Reporting

Private vulnerability reporting is a public-release blocker. When repository-host private reporting is enabled, use that mechanism with the affected revision, reproduction, impact, and safe mitigation. Until then, do not disclose vulnerabilities, secrets, or private task history publicly; the project does not claim a private channel or response time yet.
