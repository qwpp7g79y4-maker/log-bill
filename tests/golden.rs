//! Golden test: the files in `examples/` produce exactly the expected output.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("run log-bill");
    assert!(out.status.success(), "{args:?}: {:?}", out.status);
    assert!(out.stderr.is_empty(), "{args:?}: stderr not empty");
    String::from_utf8(out.stdout).expect("utf-8 output")
}

fn expected(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {name}: {err}"))
}

#[test]
fn checkout_log_matches_expected_output() {
    let got = run(&["scan", "--window", "10s", "examples/checkout.log"]);
    assert_eq!(got, expected("checkout.expected.txt"));
}

#[test]
fn api_jsonl_matches_expected_output() {
    let got = run(&[
        "scan",
        "--format",
        "jsonl",
        "--window",
        "10s",
        "examples/api.jsonl",
    ]);
    assert_eq!(got, expected("api.expected.txt"));
}
