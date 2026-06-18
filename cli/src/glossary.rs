//! `navigator glossary [term]` — print canonical Navigator term
//! definitions. With no argument, prints every term. With an
//! argument, prints just the matching term (case-insensitive); exits
//! non-zero on an unknown term so scripts can detect it.
//!
//! Term definitions intentionally live in code rather than a separate
//! YAML file — they're short, stable, and ship with the binary so the
//! command works offline with no canonical-data setup.

use std::process::ExitCode;

use crate::palette;

/// Canonical Navigator vocabulary. Pairs of `(term, definition)`,
/// presentation order preserved.
pub const TERMS: &[(&str, &str)] = &[
    (
        "Template",
        "A versioned Markdown document definition with frontmatter metadata and a body.",
    ),
    (
        "Notation",
        "A filled-in instance of a Template for a specific respondent.",
    ),
    (
        "RespondentType",
        "Whether the subject of a Notation is a `person` or an `entity`.",
    ),
    (
        "Frontmatter",
        "The YAML block at the top of a Template (between `---` delimiters).",
    ),
    (
        "Code",
        "The unique string identifier for a Template (e.g. `NDA-001`). Stable across versions.",
    ),
    (
        "Version",
        "A semantic version string (`MAJOR.MINOR.PATCH`).",
    ),
    (
        "State Machine",
        "Notation lifecycle: open → review → waitingForQuestionnaire → waitingForWorkflow → closed.",
    ),
    (
        "Questionnaire",
        "Structured questions presented to the respondent to gather facts for the Notation.",
    ),
    (
        "Workflow",
        "Automated tasks triggered after a Questionnaire is complete (e.g. PDF generation, e-signature).",
    ),
    (
        "NotationState",
        "One of: `open`, `review`, `waitingForQuestionnaire`, `waitingForWorkflow`, `closed`.",
    ),
    (
        "Person",
        "A natural person respondent (name, DOB, SSN/ITIN, address).",
    ),
    (
        "Entity",
        "A legal entity respondent (corporation, LLC, trust, etc.).",
    ),
    (
        "EntityType",
        "Classification of an Entity (e.g. `corporation`, `llc`, `trust`, `partnership`).",
    ),
    (
        "Jurisdiction",
        "Governing legal authority, expressed as an ISO 3166-2 code (e.g. `US-CA`).",
    ),
    (
        "User",
        "An authenticated system user (attorney, paralegal, or admin).",
    ),
    (
        "Project",
        "A grouping of related Notations under a common matter or client engagement.",
    ),
    (
        "Question",
        "A single prompt within a Questionnaire, with a type and optional validation rules.",
    ),
    (
        "GitRepository",
        "A version-controlled repository linked to a Project.",
    ),
    (
        "Credential",
        "Stored authentication for an external system (e.g. court filing portal, e-signature provider).",
    ),
    (
        "Mailbox",
        "Email inbox for a Project or User; receives filings, notices, and correspondence.",
    ),
    (
        "Disclosure",
        "A mandatory notice required by law, tracked for compliance.",
    ),
    (
        "PersonEntityRole",
        "The role a Person plays within an Entity (e.g. `officer`, `director`, `member`, `trustee`).",
    ),
    (
        "Document",
        "Any client-provided or generated file associated with a legal matter, owned by a Project.",
    ),
    (
        "Letter",
        "A physical piece of mail managed by a Mailroom, optionally linked to a scanned Blob.",
    ),
    (
        "ShareIssuance",
        "Records the issuance of shares in an Entity to a polymorphic shareholder (Person or Entity).",
    ),
];

/// Run the command. `term` is the optional positional argument: when
/// `None`, dump every term; when `Some`, look up case-insensitively
/// and exit `1` on miss.
#[must_use]
pub fn run(term: Option<&str>) -> ExitCode {
    let Some(needle) = term else {
        for (name, def) in TERMS {
            print_term(name, def);
        }
        return ExitCode::SUCCESS;
    };
    if let Some((name, def)) = find_term(needle) {
        print_term(name, def);
        ExitCode::SUCCESS
    } else {
        eprintln!("navigator: glossary: unknown term `{needle}`");
        eprintln!("Run `navigator glossary` to list every term.");
        ExitCode::from(1)
    }
}

fn print_term(name: &str, def: &str) {
    println!("{} {} {def}", palette::header(name), palette::dim("—"));
}

fn find_term(needle: &str) -> Option<&'static (&'static str, &'static str)> {
    let needle_lower = needle.to_ascii_lowercase();
    TERMS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&needle_lower))
}

#[cfg(test)]
mod tests {
    use super::{find_term, TERMS};

    #[test]
    fn find_term_is_case_insensitive() {
        assert!(find_term("template").is_some());
        assert!(find_term("TEMPLATE").is_some());
        assert!(find_term("Template").is_some());
    }

    #[test]
    fn find_term_returns_none_for_unknown_needle() {
        assert!(find_term("not-a-real-term").is_none());
    }

    #[test]
    fn glossary_contains_every_canonical_term() {
        // Guard against accidental term removal: the canonical
        // vocabulary is 25 names; if a future change drops one
        // without intent this test catches it.
        let expected = [
            "Template",
            "Notation",
            "RespondentType",
            "Frontmatter",
            "Code",
            "Version",
            "State Machine",
            "Questionnaire",
            "Workflow",
            "NotationState",
            "Person",
            "Entity",
            "EntityType",
            "Jurisdiction",
            "User",
            "Project",
            "Question",
            "GitRepository",
            "Credential",
            "Mailbox",
            "Disclosure",
            "PersonEntityRole",
            "Document",
            "Letter",
            "ShareIssuance",
        ];
        for name in expected {
            assert!(
                TERMS.iter().any(|(n, _)| *n == name),
                "missing canonical term: {name}",
            );
        }
        assert_eq!(TERMS.len(), expected.len(), "term count drifted");
    }

    #[test]
    fn every_definition_is_non_empty() {
        for (name, def) in TERMS {
            assert!(!def.trim().is_empty(), "definition for `{name}` is empty");
        }
    }
}
