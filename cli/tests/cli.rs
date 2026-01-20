use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

fn write_file(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write test file");
}

#[test]
fn encode_auto_detects_json() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.json");
    write_file(&input, r#"{"name":"Ada","age":37}"#);

    cargo_bin_cmd!("toon")
        .arg(&input)
        .assert()
        .success()
        .stdout("name: Ada\nage: 37");
}

#[test]
fn decode_auto_detects_toon() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.toon");
    write_file(&input, "name: Ada\nage: 37");

    let expected = "{\n  \"name\": \"Ada\",\n  \"age\": 37\n}";

    cargo_bin_cmd!("toon")
        .arg(&input)
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn encode_with_custom_delimiter() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.json");
    write_file(&input, r#"{"items":[1,2,3]}"#);

    cargo_bin_cmd!("toon")
        .arg(&input)
        .args(["--delimiter", "|"])
        .assert()
        .success()
        .stdout("items[3|]: 1|2|3");
}

#[test]
fn encode_with_stats_writes_output_and_stdout() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.json");
    write_file(&input, r#"{"items":[1,2]}"#);

    cargo_bin_cmd!("toon")
        .arg(&input)
        .arg("--stats")
        .assert()
        .success()
        .stdout(
            contains("items[2]: 1,2")
                .and(contains("Token estimates:"))
                .and(contains("Saved")),
        )
        .stderr("");
}

#[test]
fn key_folding_and_flatten_depth() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.json");
    write_file(&input, r#"{"data":{"meta":{"items":[1,2]}}}"#);

    cargo_bin_cmd!("toon")
        .arg(&input)
        .args(["--keyFolding", "safe", "--flattenDepth", "3"])
        .assert()
        .success()
        .stdout("data.meta.items[2]: 1,2");
}

#[test]
fn expand_paths_safe_decodes_folded_keys() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.toon");
    write_file(&input, "data.meta.items[2]: 1,2");

    let expected = "{\n  \"data\": {\n    \"meta\": {\n      \"items\": [\n        1,\n        2\n      ]\n    }\n  }\n}";

    cargo_bin_cmd!("toon")
        .arg(&input)
        .args(["--expandPaths", "safe"])
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn no_strict_allows_tabs_in_indentation() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.toon");
    write_file(&input, "a:\n\tb: 1");

    cargo_bin_cmd!("toon")
        .arg(&input)
        .assert()
        .failure()
        .stderr(contains("tabs not allowed in indentation"));

    let expected = "{\n  \"a\": {},\n  \"b\": 1\n}";

    cargo_bin_cmd!("toon")
        .arg(&input)
        .arg("--no-strict")
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn writes_to_output_file() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.json");
    let output = dir.path().join("output.toon");
    write_file(&input, r#"{"name":"Ada"}"#);

    cargo_bin_cmd!("toon")
        .arg(&input)
        .args(["-o", output.to_str().expect("output path")])
        .assert()
        .success()
        .stdout(
            contains("Encoded")
                .and(contains("output.toon"))
                .and(contains("→")),
        );

    let contents = fs::read_to_string(&output).expect("read output");
    assert_eq!(contents, "name: Ada");
}

#[test]
fn flatten_depth_without_key_folding_is_ignored() {
    let dir = TempDir::new().expect("tempdir");
    let input = dir.path().join("input.json");
    write_file(&input, r#"{"a":{"b":1}}"#);

    cargo_bin_cmd!("toon")
        .arg(&input)
        .args(["--flattenDepth", "2"])
        .assert()
        .success()
        .stdout("a:\n  b: 1");
}
