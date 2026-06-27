//! End-to-end tests for `navigator render <file> --out <pdf>`. Each
//! test writes a notation fixture to a tempdir, invokes the real
//! binary, and checks the produced PDF (or the refusal).

use std::fs;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

/// A minimal notation template that passes the full validation gate:
/// `title` / `respondent_type` / `code` / `confidential`, a
/// `questionnaire:` + `workflow:` (so it classifies as a notation
/// template) with the required staff review, and a clean Markdown body.
/// `output:` is `letter`; callers can override on the CLI.
const VALID: &str = "\
---
title: Test Demand
respondent_type: entity
code: test__demand
confidential: true
output: letter
questionnaire:
  BEGIN:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: staff_review
  staff_review:
    approved: END
    rejected: END
  END: {}
---

# Demand

Pay the sum of `{{amount}}` to **NEON LAW** without delay.

- First point
- Second point
";

fn write(dir: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, body).expect("write fixture");
    path
}

fn render(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(cargo_bin("navigator"))
        .arg("render")
        .args(args)
        .output()
        .expect("run navigator render")
}

#[test]
fn renders_a_letter_pdf_from_a_valid_template() {
    let work = TempDir::new().unwrap();
    let src = write(&work, "demand.md", VALID);
    let out = work.path().join("demand.pdf");
    let result = render(&[src.as_os_str(), "--out".as_ref(), out.as_os_str()]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&out).expect("pdf written");
    assert_eq!(&bytes[..4], b"%PDF", "output is not a PDF");
}

#[test]
fn cli_format_overrides_frontmatter_and_letter_is_larger_than_plain() {
    let work = TempDir::new().unwrap();
    let src = write(&work, "demand.md", VALID);

    let letter_out = work.path().join("letter.pdf");
    // `output: letter` from frontmatter — no flag.
    let letter = render(&[src.as_os_str(), "--out".as_ref(), letter_out.as_ref()]);
    assert!(letter.status.success());

    let plain_out = work.path().join("plain.pdf");
    // `--format plain` overrides the `output: letter` frontmatter.
    let plain = render(&[
        src.as_os_str(),
        "--out".as_ref(),
        plain_out.as_ref(),
        "--format".as_ref(),
        "plain".as_ref(),
    ]);
    assert!(plain.status.success());
    assert!(
        String::from_utf8_lossy(&plain.stdout).contains("Plain"),
        "override should report Plain, got: {}",
        String::from_utf8_lossy(&plain.stdout)
    );

    let letter_len = fs::read(&letter_out).unwrap().len();
    let plain_len = fs::read(&plain_out).unwrap().len();
    assert!(
        letter_len > plain_len,
        "letterhead PDF ({letter_len}) should exceed plain ({plain_len}) — logo missing?"
    );
}

#[test]
fn answer_substitutes_a_placeholder() {
    let work = TempDir::new().unwrap();
    let src = write(&work, "demand.md", VALID);
    let out = work.path().join("demand.pdf");
    let result = render(&[
        src.as_os_str(),
        "--out".as_ref(),
        out.as_ref(),
        "--answer".as_ref(),
        "amount=5000 USD".as_ref(),
    ]);
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    // The rendered PDF compresses text, so we can't grep the value out;
    // success plus a valid PDF is the contract. The substitution logic
    // itself is unit-tested in the pdf crate's markdown round-trip.
    assert_eq!(&fs::read(&out).unwrap()[..4], b"%PDF");
}

#[test]
fn renders_despite_a_non_blocking_advisory() {
    // The `VALID` fixture's mandatory `staff_review` gate earns the
    // yellow N112 "not built yet" advisory — a Warning, not an Error.
    // Rendering must not be blocked by it (it is, however, still printed
    // so the author sees it), mirroring `validate` / `import`.
    let work = TempDir::new().unwrap();
    let src = write(&work, "demand.md", VALID);
    let out = work.path().join("demand.pdf");
    let result = render(&[src.as_os_str(), "--out".as_ref(), out.as_os_str()]);
    assert!(
        result.status.success(),
        "a Warning-only template must still render, stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("N112"),
        "the advisory should still be surfaced, got stdout: {stdout}"
    );
    assert_eq!(
        &fs::read(&out).unwrap()[..4],
        b"%PDF",
        "output is not a PDF"
    );
}

#[test]
fn refuses_a_template_that_fails_validation() {
    let work = TempDir::new().unwrap();
    // Drop the required `code:` field (N108) — still classifies as a
    // notation template via its workflow, so the gate fires.
    let bad = VALID.replace("code: test__demand\n", "");
    let src = write(&work, "demand.md", &bad);
    let out = work.path().join("demand.pdf");
    let result = render(&[src.as_os_str(), "--out".as_ref(), out.as_os_str()]);
    assert!(
        !result.status.success(),
        "should refuse an invalid template"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("validation error"),
        "expected a validation refusal, got: {stderr}"
    );
    assert!(!out.exists(), "no PDF should be written on refusal");
}

#[test]
fn rejects_an_unknown_format() {
    let work = TempDir::new().unwrap();
    let src = write(&work, "demand.md", VALID);
    let out = work.path().join("demand.pdf");
    let result = render(&[
        src.as_os_str(),
        "--out".as_ref(),
        out.as_ref(),
        "--format".as_ref(),
        "demand_letter".as_ref(),
    ]);
    assert!(!result.status.success(), "unknown format should fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("unknown --format"),
        "expected unknown-format error, got: {stderr}"
    );
}
