//! Pure structural-validation tests — no database. These mirror what an
//! editor/LSP or a dry-run CLI would surface before any write.

use import::{canonical_url, parse, validate, Severity};

fn errors(payload: &import::Payload) -> Vec<String> {
    validate(payload)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{}: {}", d.pointer, d.message))
        .collect()
}

#[test]
fn canonical_url_upgrades_strips_and_lowercases() {
    assert_eq!(
        canonical_url("http://Example.ORG/?utm_source=x#frag").unwrap(),
        "https://example.org"
    );
    assert_eq!(
        canonical_url("https://nwjustice.org/").unwrap(),
        "https://nwjustice.org"
    );
    assert_eq!(
        canonical_url("https://legalaidchicago.org/about").unwrap(),
        "https://legalaidchicago.org/about"
    );
}

#[test]
fn canonical_url_rejects_schemeless_and_non_http() {
    assert!(canonical_url("nwjustice.org").is_err());
    assert!(canonical_url("ftp://files.example.org").is_err());
}

#[test]
fn valid_payload_has_no_errors() {
    let payload = parse(SAMPLE).expect("parse sample");
    assert!(
        errors(&payload).is_empty(),
        "unexpected: {:?}",
        errors(&payload)
    );
}

#[test]
fn duplicate_email_is_an_error() {
    let dup = SAMPLE.replace("mgordon@mylegalaid.org", "mmumgaard@mylegalaid.org");
    let payload = parse(&dup).expect("parse");
    assert!(errors(&payload)
        .iter()
        .any(|e| e.contains("duplicate email")));
}

#[test]
fn unknown_organization_reference_is_an_error() {
    let bad = SAMPLE.replace("\"organization\": \"njp\"", "\"organization\": \"ghost\"");
    let payload = parse(&bad).expect("parse");
    assert!(errors(&payload)
        .iter()
        .any(|e| e.contains("organization `ghost`")));
}

#[test]
fn bad_jurisdiction_code_is_an_error() {
    let bad = SAMPLE.replace(
        "\"jurisdiction\": \"WA\"",
        "\"jurisdiction\": \"Washington\"",
    );
    let payload = parse(&bad).expect("parse");
    assert!(errors(&payload).iter().any(|e| e.contains("jurisdiction")));
}

#[test]
fn noncanonical_url_is_a_warning_not_an_error() {
    let raw = SAMPLE.replace("https://nwjustice.org", "http://nwjustice.org/?ref=x");
    let payload = parse(&raw).expect("parse");
    assert!(errors(&payload).is_empty());
    assert!(validate(&payload)
        .iter()
        .any(|d| d.severity == Severity::Warning && d.message.contains("canonicalized")));
}

/// The six contacts from the original outreach list, four organizations.
const SAMPLE: &str = r#"{
  "version": 1,
  "source": "legal-aid-outreach-2026-06",
  "organizations": [
    { "key": "njp", "name": "Northwest Justice Project", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "WA", "phone": "206-464-1519", "url": "https://nwjustice.org" },
    { "key": "mmla", "name": "Mid-Minnesota Legal Aid", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "MN", "phone": "612-332-1441", "url": "https://mylegalaid.org" },
    { "key": "lac", "name": "Legal Aid Chicago", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "IL", "phone": "312-341-1070", "url": "https://legalaidchicago.org" },
    { "key": "lsnyc", "name": "Legal Services NYC", "entity_type": "501(c)(3) Non-Profit", "jurisdiction": "NY", "phone": "646-442-3600", "url": "https://lsnyc.org" }
  ],
  "people": [
    { "key": "abigail-daquiz", "name": "Abigail Daquiz", "email": "adaquiz@nwjustice.org", "title": "Executive Director", "phone": "206-464-1519", "organization": "njp" },
    { "key": "milo-mumgaard", "name": "Milo Mumgaard", "email": "mmumgaard@mylegalaid.org", "title": "Executive Director", "phone": "612-332-1441", "organization": "mmla" },
    { "key": "marv-gordon", "name": "Marv Gordon", "email": "mgordon@mylegalaid.org", "title": "IT Director", "phone": "612-332-1441", "organization": "mmla" },
    { "key": "katherine-shank", "name": "Katherine W. Shank", "email": "kshank@legalaidchicago.org", "title": "CEO and Executive Director", "phone": "312-341-1070", "organization": "lac" },
    { "key": "shervon-small", "name": "Shervon M. Small", "email": "ssmall@lsnyc.org", "title": "Executive Director", "phone": "646-442-3600", "organization": "lsnyc" },
    { "key": "dilip-kulkarni", "name": "Dilip Kulkarni", "email": "dkulkarni@lsnyc.org", "title": "Chief Information Officer", "phone": "646-442-3600", "organization": "lsnyc" }
  ]
}"#;
