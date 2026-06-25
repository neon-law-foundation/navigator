//! Grounding test for the "Form a Nevada LLC from the command line"
//! section of the Use-Neon Law Navigator workshop.
//!
//! `web/content/workshops/navigator/README.md` now teaches a concrete
//! command-line formation flow that we publish on the website. That prose
//! is a public promise — and nothing stops it drifting from the binary: a
//! renamed flag, a dropped subcommand, a template code that no longer
//! exists, a workflow that stops being staff-gated. These tests pin every
//! claim the section makes to the code that ships in this commit, the same
//! way `web/tests/deploy_workshop_auth.rs` pins the sign-in section.
//!
//! The CLI claims are grounded against the **real binary**: each command
//! the workshop names is invoked with `--help`, and every flag the prose
//! prints must appear in that help — so the published commands can't drift
//! from `navigator`'s actual interface.

use std::path::Path;
use std::process::Command;

/// Read a repo-root file relative to this crate (`cli/` → workspace root
/// is one level up).
fn repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {} — {e}", path.display()))
}

/// The body of the workshop's LLC section — everything from that heading
/// to the next `## `.
fn llc_section() -> String {
    let readme = repo_file("web/content/workshops/navigator/README.md");
    let after = readme
        .split_once("## Form a Nevada LLC from the command line")
        .expect("README.md must carry the LLC command-line section")
        .1;
    match after.split_once("\n## ") {
        Some((body, _)) => body.to_string(),
        None => after.to_string(),
    }
}

/// Run `navigator <args> --help` and return (success, help text). `--help`
/// short-circuits clap before any I/O, so this needs no server or DB.
fn help(args: &[&str]) -> (bool, String) {
    let mut invocation: Vec<&str> = args.to_vec();
    invocation.push("--help");
    let out = Command::new(env!("CARGO_BIN_EXE_navigator"))
        .args(&invocation)
        .output()
        .expect("run navigator --help");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    (out.status.success(), text)
}

#[test]
fn every_command_the_section_documents_is_a_real_navigator_subcommand() {
    let section = llc_section();
    for (phrase, args) in [
        ("matter open", ["matter", "open"].as_slice()),
        ("intake answer", ["intake", "answer"].as_slice()),
        ("notation status", ["notation", "status"].as_slice()),
        ("notation approve", ["notation", "approve"].as_slice()),
        ("notation document", ["notation", "document"].as_slice()),
        ("login", ["login"].as_slice()),
    ] {
        assert!(
            section.contains(phrase),
            "the workshop LLC section must name `navigator {phrase}`",
        );
        let (ok, _) = help(args);
        assert!(
            ok,
            "the workshop names `navigator {phrase}`, but `navigator {} --help` did not succeed — \
             the published command drifted from the binary",
            args.join(" "),
        );
    }
}

#[test]
fn every_flag_the_section_prints_appears_in_the_commands_help() {
    let section = llc_section();
    // Each command and the flags the prose teaches for it. The flag must be
    // both printed in the workshop AND present in the real `--help` output.
    for (args, flags) in [
        (
            ["matter", "open"].as_slice(),
            ["--template", "--client-email"].as_slice(),
        ),
        (
            ["intake", "answer"].as_slice(),
            ["--answer", "--person"].as_slice(),
        ),
        (["notation", "document"].as_slice(), ["--out"].as_slice()),
    ] {
        let (ok, text) = help(args);
        assert!(ok, "`navigator {} --help` failed", args.join(" "));
        for flag in flags {
            assert!(
                section.contains(flag),
                "the workshop LLC section must document the `{flag}` flag",
            );
            assert!(
                text.contains(flag),
                "the workshop prints `{flag}` for `navigator {}`, but its --help does not list it",
                args.join(" "),
            );
        }
    }
}

#[test]
fn the_template_code_the_section_names_is_a_real_seeded_template() {
    // `onboarding__nest` must be a real template code, or `matter open
    // --template onboarding__nest` (and the screenshot in the prose) is a
    // dead command.
    let section = llc_section();
    assert!(
        section.contains("onboarding__nest"),
        "the workshop must name the `onboarding__nest` template",
    );
    let template = repo_file(
        "notation_templates/united_states/nevada/state/business_associations/entity_formation.md",
    );
    assert!(
        template.contains("code: onboarding__nest"),
        "`notation_templates/united_states/nevada/state/business_associations/entity_formation.md` no longer declares `code: onboarding__nest` — \
         the workshop's `--template onboarding__nest` is now a dead command",
    );
}

#[test]
fn the_staff_gated_filing_promise_holds_in_the_workflow() {
    // The section promises the matter ends at a staff-gated
    // `filing__nv_sos` and that Neon Law Navigator never files. Bind both to the
    // bundled workflow spec: the LLC formation must actually carry that
    // filing state.
    let section = llc_section();
    assert!(
        section.contains("filing__nv_sos"),
        "the workshop must name the `filing__nv_sos` step it ends at",
    );
    assert!(
        section.to_lowercase().contains("never files"),
        "the workshop must keep the 'Neon Law Navigator never files for you' promise",
    );
    let spec = repo_file("workflows/specs/onboarding__nest.yaml");
    assert!(
        spec.contains("filing__nv_sos"),
        "`onboarding__nest.yaml` no longer reaches `filing__nv_sos` — the workshop's \
         staff-gated-filing promise has drifted from the workflow",
    );
}
