# Tasks

## Phase 0: Structure-only
- [ ] Confirm scope is structure-only and excludes any references to `src_v1` or `src_v2`.
- [ ] Scaffold the module tree defined in the design with empty files and `mod` declarations.
- [ ] Define public API stubs in `src/lib.rs` for encode/decode entry points.
- [ ] Add `src/options.rs` with `EncodeOptions` and `DecodeOptions` plus defaults.
- [ ] Add `src/error.rs` with `Error`, `ErrorKind::NotImplemented`, and minimal location fields.
- [ ] Add placeholder types in internal modules (`arena`, `tabular`, `text`, `num`) to satisfy compilation only.
- [ ] Ensure no `use` statements or dependencies reference `src_v1`, `src_v2`, or their tests.
- [ ] Create a test enablement checklist (list ignored tests to enable one by one in later phases).

## Guardrails for later phases
- [ ] Do not enable any tests until the structure compiles cleanly.
- [ ] Enable tests one at a time and implement features in isolation per test.
