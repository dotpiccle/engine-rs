//! Snapshot tests (AGENTS.md §8.5): the resolved document model for every
//! official example, and the error report (stage/code/path) for every invalid
//! fixture. Snapshots are conformance evidence; review diffs with
//! `cargo insta review`.

use std::fs;
use std::path::PathBuf;

fn spec_dir() -> PathBuf {
    std::env::var("PICCLE_SPEC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../piccle-spec"))
}

fn json_files(dir: PathBuf) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("spec directory must exist")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

#[test]
fn example_documents_resolve_to_stable_models() {
    for path in json_files(spec_dir().join("examples")) {
        let stem = path.file_stem().expect("file name").to_string_lossy().into_owned();
        let bytes = fs::read(&path).expect("example must read");
        let document = piccle_validate::validate(&bytes).expect("example must validate");
        insta::assert_debug_snapshot!(stem, document);
    }
}

#[test]
fn invalid_fixtures_produce_stable_error_reports() {
    for path in json_files(spec_dir().join("test-vectors/invalid")) {
        let stem = path.file_stem().expect("file name").to_string_lossy().into_owned();
        let bytes = fs::read(&path).expect("fixture must read");
        let error = piccle_validate::Validator::check(&bytes).expect_err("fixture must fail");
        insta::assert_snapshot!(
            stem,
            format!("{} {} {}", error.stage(), error.code(), error.path())
        );
    }
}
