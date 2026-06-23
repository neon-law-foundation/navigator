//! Cross-crate reconciliation: the firm-admission **path vocabulary** in
//! `rules::f110::JURISDICTIONS` is hand-maintained and can silently drift
//! from the canonical **jurisdiction reference data** in
//! `store/seeds/Jurisdiction.yaml`. This test closes that gap at
//! `cargo test` without putting a DB or Docker into the `validate`
//! linter, which stays a pure function over `SourceFile`.
//!
//! Division of authority:
//! - the seed is the single source of truth for *what jurisdictions exist*;
//! - `f110.rs` is the source of truth for *which subset the firm is
//!   admitted in* (a strict subset of the seed, plus reserved keywords).
//!
//! `cli` is the natural home because it already depends on both `rules`
//! and `store`.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Seed {
    records: Vec<Record>,
}

#[derive(Debug, Deserialize)]
struct Record {
    name: String,
    code: String,
    jurisdiction_type: String,
}

fn seeded() -> Vec<Record> {
    let seed: Seed = serde_yaml::from_str(store::seed::JURISDICTION_SEED_YAML)
        .expect("Jurisdiction.yaml parses against the schema-true shape");
    seed.records
}

/// `federal` has no jurisdiction row — it is a reserved scope keyword for
/// the sovereign whose row is `United States`. Every other state scope in
/// the path vocabulary must correspond to a seeded jurisdiction.
const RESERVED_SCOPES: &[&str] = &["federal"];

#[test]
fn every_jurisdiction_root_has_a_seeded_country_row() {
    let rows = seeded();
    // The single jurisdiction root is `united_states`; assert it maps to a
    // seeded country row (matched by snake_cased name and by `US` code).
    for (root, _scopes) in rules::JURISDICTIONS {
        let want_name = root.replace('_', " ");
        let row = rows
            .iter()
            .find(|r| r.name.to_lowercase() == want_name)
            .unwrap_or_else(|| {
                panic!(
                    "jurisdiction root `{root}` has no seeded row named `{want_name}` \
                     (case-insensitive); add it to store/seeds/Jurisdiction.yaml"
                )
            });
        assert_eq!(
            row.jurisdiction_type, "country",
            "jurisdiction root `{root}` should be a `country`, got `{}`",
            row.jurisdiction_type
        );
    }
}

#[test]
fn every_state_scope_corresponds_to_a_seeded_jurisdiction() {
    let rows = seeded();
    let by_snake_name: std::collections::HashSet<String> = rows
        .iter()
        .map(|r| r.name.to_lowercase().replace(' ', "_"))
        .collect();

    for (root, scopes) in rules::JURISDICTIONS {
        for scope in *scopes {
            if RESERVED_SCOPES.contains(scope) {
                continue;
            }
            assert!(
                by_snake_name.contains(*scope),
                "scope `{scope}` under `{root}/` has no backing jurisdiction row \
                 (snake_cased seed name); add it to store/seeds/Jurisdiction.yaml \
                 or remove the scope from rules::f110::JURISDICTIONS"
            );
        }
    }
}

#[test]
fn reserved_scopes_are_not_silently_added_as_rows() {
    // `federal` is reserved precisely because it is *not* a jurisdiction
    // row; if someone adds one, this surfaces the ambiguity rather than
    // letting the two sources disagree.
    let rows = seeded();
    let codes: std::collections::HashSet<&str> = rows.iter().map(|r| r.code.as_str()).collect();
    assert!(
        codes.contains("US"),
        "the `United States` row (code `US`) backs the `federal` reserved scope"
    );
    for reserved in RESERVED_SCOPES {
        let snake_names: std::collections::HashSet<String> = rows
            .iter()
            .map(|r| r.name.to_lowercase().replace(' ', "_"))
            .collect();
        assert!(
            !snake_names.contains(*reserved),
            "`{reserved}` is a reserved scope keyword and must not be a jurisdiction row; \
             it is represented by the `United States` country row"
        );
    }
}
