//! Fixture-driven conformance tests against piccle-spec/test-vectors.
//!
//! Every valid fixture must validate; every invalid fixture must fail with
//! the exact stage/code/path of invalid-expectations.json.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use piccle_validate::Validator;

fn spec_dir() -> PathBuf {
    std::env::var("PICCLE_SPEC_DIR").map_or_else(
        |_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../piccle-spec"),
        PathBuf::from,
    )
}

fn read_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

struct Expectation {
    stage: String,
    code: String,
    path: String,
}

struct FixtureReport {
    count: usize,
    failures: Vec<String>,
}

fn load_expectations(bytes: &[u8]) -> BTreeMap<String, Expectation> {
    let value: serde_json::Value = serde_json::from_slice(bytes).expect("expectations JSON");
    let obj = value.as_object().expect("expectations object");
    let mut out = BTreeMap::new();
    for (name, entry) in obj {
        let get = |key: &str| {
            entry.get(key).and_then(|v| v.as_str()).expect("expectation field").to_owned()
        };
        out.insert(
            name.clone(),
            Expectation { stage: get("stage"), code: get("code"), path: get("path") },
        );
    }
    out
}

fn validate_valid_fixtures() -> FixtureReport {
    let dir = spec_dir().join("test-vectors/valid");
    let mut failures = Vec::new();
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).expect("valid fixture dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        count += 1;
        let bytes = read_bytes(&path);
        if let Err(err) = Validator::validate(&bytes) {
            failures.push(format!(
                "{}: unexpected {} ({}) at {}",
                entry.file_name().to_string_lossy(),
                err.code(),
                err.stage(),
                err.path()
            ));
        }
    }
    FixtureReport { count, failures }
}

fn validate_invalid_fixtures() -> FixtureReport {
    let expectations_path = spec_dir().join("test-vectors/invalid-expectations.json");
    let expectations = load_expectations(&read_bytes(&expectations_path));

    let dir = spec_dir().join("test-vectors/invalid");
    let mut failures = Vec::new();
    let mut count = 0;
    for (name, expected) in &expectations {
        count += 1;
        let path = dir.join(name);
        let bytes = read_bytes(&path);
        match Validator::validate(&bytes) {
            Ok(_) => failures.push(format!("{name}: unexpectedly VALID")),
            Err(err) => {
                let actual =
                    (err.stage().to_string(), err.code().to_owned(), err.path().to_owned());
                let wanted = (expected.stage.clone(), expected.code.clone(), expected.path.clone());
                if actual != wanted {
                    failures.push(format!(
                        "{name}: expected {} / {} / {}, got {} / {} / {}",
                        wanted.0, wanted.1, wanted.2, actual.0, actual.1, actual.2
                    ));
                }
            }
        }
    }
    FixtureReport { count, failures }
}

#[test]
fn valid_fixture_set_is_not_empty() {
    let report = validate_valid_fixtures();
    assert!(report.count > 0, "no valid fixtures found");
}

#[test]
fn valid_fixtures_all_validate() {
    let report = validate_valid_fixtures();
    assert!(report.failures.is_empty(), "valid fixture failures:\n{}", report.failures.join("\n"));
}

#[test]
fn invalid_fixture_expectations_are_not_empty() {
    let report = validate_invalid_fixtures();
    assert!(report.count > 0, "no expectations loaded");
}

#[test]
fn invalid_fixtures_match_expectations() {
    let report = validate_invalid_fixtures();
    assert!(
        report.failures.is_empty(),
        "invalid fixture failures:\n{}",
        report.failures.join("\n")
    );
}
