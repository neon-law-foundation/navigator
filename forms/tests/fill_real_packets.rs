//! Fill the real vendored packets — the canonical-example guard in action.
//!
//! These tests exercise `pdf::fill_acroform` against the exact bytes we
//! file with the Nevada Secretary of State, covering every field shape
//! the packets carry: text fields with `OmniForm`'s hostile names, kid-less
//! checkboxes with arbitrary on-states, radio groups with `/T`-less kids,
//! and `Tx` parents whose two kid widgets print the same value on two
//! pages. If a future re-vendor renames a field or restructures a group,
//! these fail in CI — before a mis-filled packet reaches an attorney.

use std::collections::BTreeMap;

fn llc_bytes() -> &'static [u8] {
    forms::get("nv_sos__llc_formation")
        .expect("registry loads")
        .expect("LLC packet vendored")
        .bytes
}

fn fill(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[test]
fn llc_packet_fills_every_field_shape() {
    let fields = fill(&[
        // Text field, hostile OmniForm name (Articles of Organization).
        (
            "1 Name of Entity If foreign name in home jurisdiction",
            "Neon Demo LLC",
        ),
        // Kid-less checkbox, arbitrary on-state (management structure).
        ("managers_a", "managers"),
        // Radio group with /T-less kids (processing request).
        (
            "Processing request  Select one (1) from the following 6 optopns",
            "Regular",
        ),
        // Tx parent with two duplicate kid widgets (City on two pages).
        ("City", "Las Vegas"),
    ]);
    let filled = pdf::fill_acroform(llc_bytes(), &fields).expect("fill the real LLC packet");

    for (name, want) in [
        (
            "1 Name of Entity If foreign name in home jurisdiction",
            "Neon Demo LLC",
        ),
        ("managers_a", "managers"),
        (
            "Processing request  Select one (1) from the following 6 optopns",
            "Regular",
        ),
        ("City", "Las Vegas"),
    ] {
        assert_eq!(
            pdf::read_field_value(&filled, name).as_deref(),
            Some(want),
            "round-trip of `{name}`"
        );
    }

    // The radio's chosen state must be visible (/AS), not just stored
    // (/V): the first kid carries `Regular` in the vendored packet.
    assert_eq!(
        pdf::read_widget_appearance_state(
            &filled,
            "Processing request  Select one (1) from the following 6 optopns",
            Some(0),
        )
        .as_deref(),
        Some("Regular")
    );
    // The checkbox widget itself must show checked.
    assert_eq!(
        pdf::read_widget_appearance_state(&filled, "managers_a", None).as_deref(),
        Some("managers")
    );
}

#[test]
fn llc_packet_rejects_a_wrong_checkbox_state_loudly() {
    let err = pdf::fill_acroform(llc_bytes(), &fill(&[("managers_a", "Yes")])).unwrap_err();
    match err {
        pdf::PdfError::InvalidChoice { field, allowed, .. } => {
            assert_eq!(field, "managers_a");
            assert_eq!(allowed, vec!["managers"]);
        }
        other => panic!("expected InvalidChoice, got {other:?}"),
    }
}

#[test]
fn llc_packet_rejects_a_misspelled_field_name_loudly() {
    let err =
        pdf::fill_acroform(llc_bytes(), &fill(&[("Name of Entity", "Neon Demo LLC")])).unwrap_err();
    assert!(matches!(err, pdf::PdfError::UnmatchedField(name) if name == "Name of Entity"));
}

#[test]
fn all_three_packets_parse_and_expose_an_acroform() {
    for form in forms::registry().expect("registry loads") {
        // An empty fill is a parse + AcroForm-locate pass over the real
        // bytes — XFA or a parse regression fails here.
        pdf::fill_acroform(form.bytes, &BTreeMap::new())
            .unwrap_or_else(|e| panic!("{}: {e}", form.meta.form_code));
    }
}

#[test]
fn every_mapped_field_name_exists_in_the_vendored_bytes() {
    for form in forms::registry().expect("registry loads") {
        let map = forms::field_map(&form.meta.form_code)
            .expect("map parses")
            .expect("every vendored form has a map");
        let names = pdf::field_names(form.bytes).expect("field names readable");
        for rule in &map.field {
            assert!(
                names.iter().any(|n| n == &rule.name),
                "{}: mapped field `{}` does not exist in the vendored bytes — \
                 the map was guessed or the form was re-vendored without updating it",
                form.meta.form_code,
                rule.name
            );
        }
    }
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

/// Resolve a form's map against sample answers, fill the real bytes,
/// and read back every resolved value. The full pipeline a notation
/// will run in production, against the exact bytes we file.
fn round_trip(form_code: &str, answers: &BTreeMap<String, String>) {
    let form = forms::get(form_code)
        .expect("registry loads")
        .expect("form vendored");
    let map = forms::field_map(form_code)
        .expect("map parses")
        .expect("map exists");
    let resolved = forms::resolve(&map, answers).expect("answers resolve");
    assert!(
        !resolved.is_empty(),
        "{form_code}: sample answers resolved to nothing"
    );
    let filled = pdf::fill_acroform(form.bytes, &resolved).expect("fill succeeds");
    let read_back = pdf::read_field_values(&filled).expect("filled packet re-parses");
    for (name, want) in &resolved {
        assert_eq!(
            read_back.get(name),
            Some(want),
            "{form_code}: `{name}` did not round-trip"
        );
    }
}

#[test]
fn llc_map_round_trips_through_the_real_packet() {
    let answers: BTreeMap<String, String> = [
        ("entity_name", "Neon Demo LLC".to_string()),
        ("registered_agent", "Neon Law Services".to_string()),
        ("management_structure", "members".to_string()),
        ("managing_members", two_people()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    round_trip("nv_sos__llc_formation", &answers);
}

#[test]
fn corp_map_round_trips_through_the_real_packet() {
    let answers: BTreeMap<String, String> = [
        ("entity_name", "Neon Demo Corp".to_string()),
        ("registered_agent", "Neon Law Services".to_string()),
        ("shares_authorized", "1000".to_string()),
        ("par_value", "0.01".to_string()),
        ("directors", two_people()),
        ("corporate_officers", two_people()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    round_trip("nv_sos__profit_corp_formation", &answers);
}

#[test]
fn business_trust_map_round_trips_through_the_real_packet() {
    let answers: BTreeMap<String, String> = [
        ("entity_name", "Neon Demo Trust".to_string()),
        ("registered_agent", "Neon Law Services".to_string()),
        ("trustees", two_people()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    round_trip("nv_sos__business_trust_formation", &answers);
}

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
    let map = forms::field_map("nv_sos__llc_formation").unwrap().unwrap();
    let resolved = forms::resolve(&map, &answers).unwrap();
    assert_eq!(resolved["Title"], "Managing Member");
    assert!(
        !resolved.contains_key("Title_2"),
        "an empty officer slot must not carry a printed title"
    );
    assert!(!resolved.contains_key("Name_2"));
}
