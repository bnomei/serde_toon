use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use serde_toon::{decode_to_value_with_options, DecodeOptions, ExpandPaths};

#[derive(Debug, Deserialize)]
struct FixtureFile {
    tests: Vec<FixtureCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureCase {
    name: String,
    input: Value,
    expected: Value,
    #[serde(default)]
    should_error: bool,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixture_file(path: &Path) -> FixtureFile {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()));
    serde_json::from_str(&contents)
        .unwrap_or_else(|err| panic!("failed to parse fixture {}: {err}", path.display()))
}

#[test]
fn arena_decode_matches_tabular_fixtures() {
    let path = fixture_root().join("decode/arrays-tabular.json");
    let fixture = load_fixture_file(&path);
    let options = DecodeOptions::new().with_expand_paths(ExpandPaths::Off);

    for case in fixture.tests {
        let name = format!("{}::{}", path.display(), case.name);
        let input = case
            .input
            .as_str()
            .unwrap_or_else(|| panic!("decode input must be a string for {name}"));
        if case.should_error {
            assert!(
                decode_to_value_with_options(input, &options).is_err(),
                "expected error for {name}"
            );
            continue;
        }
        let actual = decode_to_value_with_options(input, &options)
            .unwrap_or_else(|err| panic!("decode failed for {name}: {}", err.message));
        assert_eq!(actual, case.expected, "decode mismatch for {name}");
    }
}
