# Sandboxing

Security profiles are explicit and capability-based. Higher-isolation profiles deny more capabilities and expose less host state. Elevated-capability profiles explicitly weaken isolation or grant capabilities; they are not called stronger. The baseline denies network, display, SSH, D-Bus, Docker, and unrelated host access. A capability matrix and final profile names belong in the secure-worktree OpenSpec change.

Isolation layers are complementary: bubblewrap constructs namespaces and mounts, Landlock restricts filesystem access, and seccomp restricts syscalls. The task receives a private cleared environment and private task home. Inherited file descriptors are enumerated, closed or explicitly passed, and verified. Git integration remains daemon-owned in the task integration worktree; logical child writer worktrees are sibling directories.

Network access is unavailable by default and any exception is explicit, bounded, and evidenced. WSL and AppArmor diagnostics must distinguish unavailable kernel features from policy denial. No silent fallback is allowed. ai-jail may inform threat analysis but is GPL reference material only and is not a dependency.

Verification categories are capability allow/deny tests, filesystem and process escape tests, inherited-FD tests, resource-limit tests, profile-failure tests, worktree ownership tests, WSL/headless qualification, and crash cleanup tests.
