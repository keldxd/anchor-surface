//! End-to-end tests: run the real binary on the fixtures in tests/fixtures.

use anchor_surface::json::{self, Value};
use std::process::Command;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_anchor-surface"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn json_report(old: &str, new: &str) -> (i32, Value) {
    let (code, stdout, _) = run(&[&fixture(old), &fixture(new), "--format", "json"]);
    (
        code,
        json::parse(&stdout).expect("--format json must print valid JSON"),
    )
}

fn rules(report: &Value) -> Vec<(String, String)> {
    report
        .get("findings")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f.get("severity").unwrap().as_str().unwrap().to_string(),
                f.get("rule").unwrap().as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn same_idl_twice_is_clean() {
    let (code, report) = json_report("vault_v1", "vault_v1");
    assert_eq!(code, 0);
    assert!(rules(&report).is_empty());
}

#[test]
fn every_planted_loosening_is_caught() {
    let (code, report) = json_report("vault_v1", "vault_v2_planted");
    assert_eq!(code, 1, "danger findings must fail the run by default");
    let r = rules(&report);
    for expected in [
        ("danger", "address-removed"),
        ("danger", "pda-changed"),
        ("danger", "signer-removed"),
        ("danger", "relation-removed"),
        ("review", "writable-added"),
        ("review", "instruction-added"),
    ] {
        assert!(
            r.contains(&(expected.0.to_string(), expected.1.to_string())),
            "missing {expected:?} in {r:?}"
        );
    }
    // Two relations were removed (withdraw/position and set_fee/config).
    assert_eq!(
        r.iter()
            .filter(|(_, rule)| rule == "relation-removed")
            .count(),
        2
    );
    assert_eq!(r.len(), 7, "unexpected extra findings: {r:?}");

    let summary = report.get("summary").unwrap();
    assert_eq!(summary.get("danger").unwrap(), &Value::Number("5".into()));
    assert_eq!(summary.get("review").unwrap(), &Value::Number("2".into()));
}

#[test]
fn safe_upgrade_has_no_danger_and_passes() {
    let (code, report) = json_report("vault_v1", "vault_v2_safe");
    assert_eq!(code, 0);
    let r = rules(&report);
    assert!(r.iter().all(|(sev, _)| sev == "info"), "{r:?}");
    assert_eq!(r.len(), 2);
}

#[test]
fn legacy_to_modern_matches_names_and_says_what_it_skipped() {
    let (code, report) = json_report("vault_v1_legacy", "vault_v1");
    assert_eq!(code, 0);
    assert!(
        rules(&report).is_empty(),
        "camelCase legacy names must match snake_case names"
    );
    assert_eq!(
        report.get("constraints_compared").unwrap(),
        &Value::Bool(false)
    );
    assert_eq!(report.get("notes").unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn fail_on_controls_the_exit_code() {
    let (old, new) = (fixture("vault_v1"), fixture("vault_v2_safe"));
    assert_eq!(run(&[&old, &new]).0, 0);
    assert_eq!(run(&[&old, &new, "--fail-on", "info"]).0, 1);
    let planted = fixture("vault_v2_planted");
    assert_eq!(run(&[&old, &planted, "--fail-on", "never"]).0, 0);
}

#[test]
fn text_output_states_the_limits() {
    let (_, stdout, _) = run(&[&fixture("vault_v1"), &fixture("vault_v2_planted")]);
    assert!(stdout.contains("DANGER  withdraw / owner  [signer-removed]"));
    assert!(stdout.contains("5 danger, 2 review, 0 info"));
    assert!(stdout.contains("no findings does not mean the upgrade is safe"));
}

#[test]
fn bad_input_exits_with_2() {
    let (code, _, stderr) = run(&["/no/such/file.json", &fixture("vault_v1")]);
    assert_eq!(code, 2);
    assert!(stderr.contains("cannot read"));

    let not_idl = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    assert_eq!(run(&[&not_idl, &fixture("vault_v1")]).0, 2);
    assert_eq!(run(&[&fixture("vault_v1")]).0, 2);
}

#[test]
fn help_and_version() {
    let (code, stdout, _) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("EXIT CODES"));
    let (code, stdout, _) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("anchor-surface "));
}
