//! Fill round-trip through the storage seam — the pipeline a notation
//! runs in production, sourced the way production sources it.
//!
//! The canonical blanks live only in the public assets bucket, so these
//! tests stage a synthetic blank per vendored form (built from its own
//! `.fields.toml`, so every mapped field shape exists) in a
//! `fake-gcs-server` container, then run the full production pipeline
//! against the `cloud::StorageService` seam: pull → verify the sha-256
//! pin → resolve the map → `pdf::fill_acroform` → `pdf::flatten`. The
//! GCS wire path is the real one (`cloud::GcsStorage`); only the
//! emulator endpoint differs. Whether the *bucket's* bytes are the
//! pinned canonical blanks is `navigator forms sync`'s verify half —
//! network truth, checked at vendor time, not here.

use std::collections::BTreeMap;

use cloud::StorageService;

/// A synthetic, genuinely fillable blank for `form_code`, with one
/// widget per `.fields.toml` rule: a checkbox (with the rule's
/// on-state) for `checked_when`-shaped rules, a text field otherwise.
fn synthetic_blank(form_code: &str) -> Vec<u8> {
    let map = forms::field_map(form_code)
        .expect("map parses")
        .expect("every vendored form has a map");
    let mut seen = std::collections::BTreeSet::new();
    let specs: Vec<pdf::FieldSpec> = map
        .field
        .iter()
        .filter(|rule| seen.insert(rule.name.clone()))
        .map(|rule| match (&rule.checked_when, &rule.on_state) {
            (Some(_), Some(on_state)) => pdf::FieldSpec::Checkbox {
                name: rule.name.clone(),
                on_state: on_state.clone(),
            },
            _ => pdf::FieldSpec::Text {
                name: rule.name.clone(),
            },
        })
        .collect();
    pdf::blank_acroform_with(&specs)
}

fn two_people() -> String {
    r#"[
        {"name": "Aries Client", "street": "1 Main St", "city": "Las Vegas",
         "state": "NV", "zip": "89101", "country": "USA", "title": "President"},
        {"name": "Libra Partner", "street": "2 Side St", "city": "Reno",
         "state": "NV", "zip": "89501", "country": "USA", "title": "Secretary"}
    ]"#
    .to_string()
}

fn sample_answers(form_code: &str) -> BTreeMap<String, String> {
    let pairs: Vec<(&str, String)> = match form_code {
        "nv__llc_formation" => vec![
            ("entity__company.name", "Neon Demo LLC".to_string()),
            (
                "person__registered_agent.name",
                "Neon Law Services".to_string(),
            ),
            ("management_structure", "members".to_string()),
            ("managing_members", two_people()),
        ],
        "nv__profit_corp_formation" => vec![
            ("entity__company.name", "Neon Demo Corp".to_string()),
            (
                "person__registered_agent.name",
                "Neon Law Services".to_string(),
            ),
            ("shares_authorized", "1000".to_string()),
            ("par_value", "0.01".to_string()),
            ("directors", two_people()),
            ("corporate_officers", two_people()),
        ],
        "nv__business_trust_formation" => vec![
            ("entity__company.name", "Neon Demo Trust".to_string()),
            (
                "person__registered_agent.name",
                "Neon Law Services".to_string(),
            ),
            ("trustees", two_people()),
        ],
        other => panic!("no sample answers for `{other}`"),
    };
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// One emulator, every vendored form, the whole pipeline: stage the
/// blank, pull it back through the trait, verify its pin, resolve the
/// map, fill, read every resolved value back, then flatten and confirm
/// nothing interactive survives while the filled text does.
#[tokio::test]
async fn every_packet_round_trips_from_the_storage_seam() {
    let gcs = cloud::test_support::fake_gcs("navigator").await;
    let storage: &dyn StorageService = &gcs.storage;

    for form in forms::registry().expect("registry loads") {
        let staged = synthetic_blank(form.code);
        storage
            .put(form.object_path, &staged, "application/pdf")
            .await
            .expect("stage blank in the bucket");
        let pin = forms::sha256_hex(&staged);

        // Pull through the seam and verify before filling — the exact
        // gate `web::retainer_walk::acroform_payload` runs.
        let blank = storage.get(form.object_path).await.expect("pull blank");
        forms::verify_sha256(&pin, &blank.bytes).expect("staged bytes verify against their pin");

        let map = forms::field_map(form.code)
            .expect("map parses")
            .expect("map exists");
        let resolved = forms::resolve(&map, &sample_answers(form.code)).expect("answers resolve");
        assert!(
            !resolved.is_empty(),
            "{}: sample answers resolved to nothing",
            form.code
        );

        let filled = pdf::fill_acroform(&blank.bytes, &resolved).expect("fill succeeds");
        let read_back = pdf::read_field_values(&filled).expect("filled packet re-parses");
        for (name, want) in &resolved {
            assert_eq!(
                read_back.get(name),
                Some(want),
                "{}: `{name}` did not round-trip",
                form.code
            );
        }

        // Flatten freezes what staff approved: no interactive field or
        // widget survives, and the filled text is static page content.
        let flat = pdf::flatten(&filled).expect("flatten succeeds");
        assert!(
            pdf::field_names(&flat).expect("field names").is_empty(),
            "{}: flattened packet still exposes interactive fields",
            form.code
        );
        assert_eq!(
            pdf::widget_annotation_count(&flat).expect("widget count"),
            0,
            "{}: flattened packet still carries widget annotations",
            form.code
        );
        let text = pdf::page_text(&flat).expect("extract flattened page text");
        for value in ["Neon Law Services", "Aries Client"] {
            assert!(
                text.contains(value),
                "{}: flattened packet lost `{value}`",
                form.code
            );
        }
    }
}

/// The pin is the gate: bytes that don't match it must never be filled,
/// and a blank missing from the bucket is a loud error, not a fallback.
#[tokio::test]
async fn tampered_or_missing_blanks_fail_loudly_before_any_fill() {
    let gcs = cloud::test_support::fake_gcs("navigator").await;
    let storage: &dyn StorageService = &gcs.storage;
    let form = forms::get("nv__llc_formation")
        .expect("registry loads")
        .expect("LLC packet vendored");

    // Missing object: the pull itself errors.
    let err = storage.get(form.object_path).await.unwrap_err();
    assert!(
        matches!(
            err,
            cloud::StorageError::NotFound(_) | cloud::StorageError::Gcs { .. }
        ),
        "missing blank must surface as an error, got {err:?}"
    );

    // Staged bytes that fail the pin: verification refuses them.
    let staged = synthetic_blank(form.code);
    let pin = forms::sha256_hex(&staged);
    storage
        .put(form.object_path, b"%PDF-1.5 re-vendored", "application/pdf")
        .await
        .expect("stage tampered bytes");
    let pulled = storage.get(form.object_path).await.expect("pull");
    let err = forms::verify_sha256(&pin, &pulled.bytes).unwrap_err();
    assert_eq!(err.pinned, pin);
    assert_ne!(err.actual, err.pinned);
}

/// Map-resolution behavior that needs no bytes at all: filled slots and
/// their printed titles follow the people-list presence gates.
#[test]
fn single_member_llc_leaves_empty_slots_and_their_titles_blank() {
    let one_person = r#"[{"name": "Pisces Founder", "street": "9 Quiet Rd",
        "city": "Henderson", "state": "NV", "zip": "89002", "country": "USA"}]"#;
    let answers: BTreeMap<String, String> = [
        ("entity_name", "Solo Founder LLC".to_string()),
        ("registered_agent", "Neon Law Services".to_string()),
        ("management_structure", "members".to_string()),
        ("managing_members", one_person.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let map = forms::field_map("nv__llc_formation").unwrap().unwrap();
    let resolved = forms::resolve(&map, &answers).unwrap();
    assert_eq!(resolved["Title"], "Managing Member");
    assert!(
        !resolved.contains_key("Title_2"),
        "an empty officer slot must not carry a printed title"
    );
    assert!(!resolved.contains_key("Name_2"));
}

/// The fill itself still rejects bad inputs loudly — a wrong checkbox
/// state and a misspelled field name are `pdf` errors, not silent
/// blanks. (Field-shape truth for the canonical blanks is `navigator
/// forms sync`/`fields` territory; the shapes here come from the map.)
#[test]
fn wrong_states_and_misspelled_names_are_loud_fill_errors() {
    let blank = synthetic_blank("nv__llc_formation");
    let fill = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    };
    let err = pdf::fill_acroform(&blank, &fill(&[("managers_a", "Yes")])).unwrap_err();
    match err {
        pdf::PdfError::InvalidChoice { field, allowed, .. } => {
            assert_eq!(field, "managers_a");
            assert_eq!(allowed, vec!["managers"]);
        }
        other => panic!("expected InvalidChoice, got {other:?}"),
    }
    let err =
        pdf::fill_acroform(&blank, &fill(&[("Name of Entity", "Neon Demo LLC")])).unwrap_err();
    assert!(matches!(err, pdf::PdfError::UnmatchedField(name) if name == "Name of Entity"));
}
