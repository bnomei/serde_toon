use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use serde_toon::canonical::profile::CanonicalDelimiter;
use serde_toon::{decode_to_value, encode_canonical, CanonicalProfile};

#[derive(Debug, Deserialize)]
struct FixtureFile {
    version: String,
    category: String,
    description: String,
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
    options: Option<FixtureOptions>,
    min_spec_version: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct FixtureOptions {
    delimiter: Option<String>,
    indent: Option<usize>,
    key_folding: Option<String>,
    flatten_depth: Option<usize>,
    strict: Option<bool>,
    expand_paths: Option<String>,
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

fn load_fixture_dir(category: &str) -> Vec<(PathBuf, FixtureFile)> {
    let root = fixture_root().join(category);
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("failed to read fixture dir {}: {err}", root.display()))
    {
        let entry = entry
            .unwrap_or_else(|err| panic!("failed to read fixture dir {}: {err}", root.display()));
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        entries.push((path.clone(), load_fixture_file(&path)));
    }
    entries.sort_by_key(|(path, _)| path.file_name().map(|name| name.to_os_string()));
    entries
}

fn encode_profile(options: Option<&FixtureOptions>) -> CanonicalProfile {
    let mut profile = CanonicalProfile::default();
    if let Some(options) = options {
        if let Some(indent) = options.indent {
            profile.indent_spaces = indent;
        }
        if let Some(delimiter) = options.delimiter.as_deref() {
            profile.delimiter = match delimiter {
                "," => CanonicalDelimiter::Comma,
                "\t" => CanonicalDelimiter::Tab,
                "|" => CanonicalDelimiter::Pipe,
                _ => panic!("unsupported delimiter in fixture options: {delimiter:?}"),
            };
        }
    }
    profile
}

fn supports_encode(options: Option<&FixtureOptions>) -> bool {
    let Some(options) = options else {
        return true;
    };
    if let Some(key_folding) = options.key_folding.as_deref() {
        if key_folding != "off" {
            return false;
        }
    }
    options.flatten_depth.is_none()
}

fn supports_decode(options: Option<&FixtureOptions>) -> bool {
    let Some(options) = options else {
        return true;
    };
    if let Some(indent) = options.indent {
        if indent != 2 {
            return false;
        }
    }
    if let Some(strict) = options.strict {
        if !strict {
            return false;
        }
    }
    if let Some(expand_paths) = options.expand_paths.as_deref() {
        if expand_paths != "off" {
            return false;
        }
    }
    true
}

#[rstest::rstest]
fn conformance_encode_fixtures() {
    let mut executed = 0;
    for (path, fixture) in load_fixture_dir("encode") {
        for case in fixture.tests {
            if !supports_encode(case.options.as_ref()) {
                continue;
            }
            executed += 1;
            let name = format!("{}::{}", path.display(), case.name);
            let profile = encode_profile(case.options.as_ref());
            if case.should_error {
                assert!(
                    encode_canonical(&case.input, profile).is_err(),
                    "expected error for {name}"
                );
                continue;
            }
            let expected = case
                .expected
                .as_str()
                .unwrap_or_else(|| panic!("encode expected must be a string for {name}"));
            let actual = encode_canonical(&case.input, profile)
                .unwrap_or_else(|err| panic!("encode failed for {name}: {err}"));
            assert_eq!(actual, expected, "encode mismatch for {name}");
        }
    }
    assert!(executed > 0, "no encode fixtures executed");
}

#[rstest::rstest]
fn conformance_decode_fixtures() {
    let mut executed = 0;
    for (path, fixture) in load_fixture_dir("decode") {
        for case in fixture.tests {
            if !supports_decode(case.options.as_ref()) {
                continue;
            }
            executed += 1;
            let name = format!("{}::{}", path.display(), case.name);
            let input = case
                .input
                .as_str()
                .unwrap_or_else(|| panic!("decode input must be a string for {name}"));
            if case.should_error {
                assert!(decode_to_value(input).is_err(), "expected error for {name}");
                continue;
            }
            let expected = case.expected;
            let actual = decode_to_value(input)
                .unwrap_or_else(|err| panic!("decode failed for {name}: {}", err.message));
            assert_eq!(actual, expected, "decode mismatch for {name}");
        }
    }
    assert!(executed > 0, "no decode fixtures executed");
}
