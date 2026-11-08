use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn fixture_input(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .join("input.csv")
}

/// Returns the header and the remaining rows sorted, since row order is unspecified.
fn header_and_sorted_rows(csv: &str) -> (String, Vec<String>) {
    let mut lines = csv.lines().map(str::to_owned);
    let header = lines.next().unwrap_or_default();
    let mut rows: Vec<String> = lines.filter(|line| !line.is_empty()).collect();
    rows.sort();
    (header, rows)
}

fn check_fixture(name: &str) {
    let input = fixture_input(name);
    let expected =
        fs::read_to_string(input.with_file_name("expected.csv")).expect("expected.csv is readable");

    let assert = cargo_bin_cmd!("transaction_parser")
        .arg(&input)
        .assert()
        .success();
    let actual = String::from_utf8(assert.get_output().stdout.clone()).expect("stdout is UTF-8");

    assert_eq!(
        header_and_sorted_rows(&actual),
        header_and_sorted_rows(&expected),
        "fixture `{name}`"
    );
}

#[test]
fn readme_example() {
    check_fixture("readme_example");
}

#[test]
fn deposits_and_withdrawals() {
    check_fixture("deposits_and_withdrawals");
}

#[test]
fn dispute_and_resolve() {
    check_fixture("dispute_and_resolve");
}

#[test]
fn chargeback_locks_account() {
    check_fixture("chargeback_locks_account");
}

#[test]
fn dispute_rules() {
    check_fixture("dispute_rules");
}

#[test]
fn negative_balance() {
    check_fixture("negative_balance");
}

#[test]
fn missing_amount_column() {
    check_fixture("missing_amount_column");
}

#[test]
fn precision() {
    check_fixture("precision");
}

#[test]
fn invalid_rows_skipped() {
    check_fixture("invalid_rows_skipped");
}

#[test]
fn empty_input() {
    check_fixture("empty_input");
}

#[test]
fn limitation_ignored_dispute_creates_account() {
    check_fixture("limitation_ignored_dispute_creates_account");
}

#[test]
fn limitation_locked_account_rejects_resolve() {
    check_fixture("limitation_locked_account_rejects_resolve");
}

#[test]
fn missing_argument_fails() {
    cargo_bin_cmd!("transaction_parser")
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[test]
fn missing_file_fails() {
    cargo_bin_cmd!("transaction_parser")
        .arg(fixture_input("does_not_exist"))
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[test]
fn limitation_unparsable_row_stops_run() {
    cargo_bin_cmd!("transaction_parser")
        .arg(fixture_input("limitation_unparsable_row_stops_run"))
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("execution failed"));
}
