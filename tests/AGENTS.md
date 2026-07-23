# Test guidance

- Executable-level external contract tests use the binary; crate-private
  capability behavior uses unit tests in or under its owning module. Do not
  promise or require a public Rust SDK. White-box schema and migration tests
  belong with their owner or a narrowly bounded harness.
- Keep fixtures in `tests/fixtures/`. Tests may deliberately inspect persisted
  contracts, but must not force production internals public.
- Do not add `#[path]` source inclusion. Preserve current includes only until
  the planned real crate-boundary migration removes them.
