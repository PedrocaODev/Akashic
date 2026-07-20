# Lifecycle and outcomes

This is an explanatory baseline; OpenSpec changes define normative transitions.

States proceed through `preflight`, `contracted`, optional `research`, `planned`, `awaiting_approval`, `implementing`, `reviewing`, `verifying`, `verification_passed`, `retrospecting`, `awaiting_acceptance`, and `delivering`. `verification_passed` is nonterminal: it still requires retrospective and human acceptance. Retrospective completion precedes acceptance and closure; post-delivery follow-up is separate.

Transitions require the prior state’s evidence and applicable supervised or guarded gate. Human approval binds to the task contract, plan, security profile, provider selection, and relevant artifact identities. A material change, invalidated evidence, changed policy, or changed inputs invalidates approval and returns the task to the applicable gate. `waiting_blocked` is resumable; terminal `blocked` is assigned only when a human closes the task without satisfying the prerequisite.

Reviewer findings are `open`, `acknowledged`, `fixed`, `waived`, `rejected`, or `stale`; unresolved findings block acceptance unless an explicit waiver is allowed. When fixes are exhausted, the task must choose a bounded outcome rather than loop: revise plan, accept with waiver, accept partially, abort, or fail. The daemon records the actor and reason. An authorized human assigns `aborted` when deliberately stopping without claiming success; the runtime assigns `failed` when execution or required verification fails and no eligible waiver or partial acceptance is granted.

- **verified:** required checks passed with evidence and no disallowed open findings.
- **accepted_with_waivers:** a human accepted documented residual findings.
- **accepted_partial:** a human accepted only a clearly bounded subset.
- **blocked:** progress cannot continue without a missing prerequisite or decision.
- **aborted:** an authorized actor stopped the task without claiming success.
- **failed:** execution or required verification failed without an accepted waiver/partial boundary.

Delivery eligibility is: `verified`; `accepted_with_waivers` with recorded rationale; or `accepted_partial` for an explicitly bounded subset. `blocked`, `aborted`, and `failed` cannot use normal delivery; they may only produce diagnostics, export, retain, or discard under the applicable human decision. Human acceptance assigns the terminal eligible outcome after `verification_passed` and retrospective. A retrospective cannot be skipped because work is partial or blocked.
