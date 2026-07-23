# Retrospective status: VERIFICATION COMPLETE WITH WAIVER

This retrospective records implementation work, local checks, and successful
hosted CI. Verification is complete with the documented waiver: cross-UID peer
rejection was conditionally skipped when `setpriv` could not perform UID
switching. Human acceptance is pending and is not evidenced in this repository.
Explicit delivery remains blocked pending human acceptance.

## What shipped
- The minimal Rust executable with daemon, TUI, JSONL, doctor, and version
  modes, configuration precedence, structured errors, bounded shutdown, secure
  Linux runtime paths, peer authorization, and the bootstrap health protocol.
- Focused Rust tests, CI wiring, Markdown/link checks, parsed OpenSpec artifact
  retention, and local verification evidence.

## What went well
- Test-first slices exposed CLI/configuration, framing, identity, cleanup, and
  stream-discipline defects before the corresponding fixes were retained.
- The bootstrap stayed dependency-light: structural JSON parsing, bounded
  framing, Linux primitives, and standard Cargo checks are sufficient.
- Security and protocol behavior remain explicit: no task/provider functionality
  was added, and unsupported non-Linux secure-runtime targets are not silently
  downgraded.

## What to watch
- Early path-based runtime checks and cleanup were vulnerable to replacement
  races; descriptor-anchored traversal, lock ownership, quarantine handling,
  and fail-closed paths were added in response.
- WSL2 cannot always perform UID switching through `setpriv`; the unauthorized
  peer test therefore has an explicit conditional qualification rather than a
  fabricated pass.
- Before public-v1 qualification or delivery, successful cross-UID peer
  rejection MUST be requalified on a Linux environment capable of UID
  switching. The current waiver remains visible until that requalification;
  no success claim is made for the branch.
- The bootstrap initially used permissive hand-written JSON and lost partial
  frames across read timeouts; structural parsing and persistent frame state
  corrected those failures.

## Follow-ups
- Obtain human acceptance before making the explicit delivery decision.
- Keep Slice 6 evidence separate from future task, provider, sandbox, storage,
  and non-Linux portability work.
