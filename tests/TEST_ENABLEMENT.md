# Test enablement checklist (v3 TDD)

Goal: enable tests one by one as features land, without pulling in v1/v2 code.

Tracking approach:
- Keep this list coarse (per file), and use per-test `#[ignore]` counts as needed.
- Enable a single ignored test at a time and implement only what that test requires.

Checklist (by file):
- [ ] tests/conformance.rs (ignored count: 0; missing canonical APIs)
- [ ] tests/spec_02_data_model.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_03_encoding_normalization.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_04_decoding_interpretation.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_05_concrete_syntax_root.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_06_header_syntax.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_07_strings_keys.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_08_objects.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_09_arrays.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_10_objects_list_items.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_11_delimiters.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_12_indentation_whitespace.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_13_conformance_and_options.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_14_strict_mode.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_15_security.rs (encoder+decoder+validator enabled; ignored count: 0)
- [ ] tests/spec_16_internationalization.rs (encoder+decoder+validator enabled; ignored count: 0)
