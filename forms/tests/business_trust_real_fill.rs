//! Fill the real re-authored NV Business Trust blank and assert every
//! resolved value round-trips through the actual bytes. Ignored by default:
//! the canonical blank lives in the assets bucket, so point the test at a
//! working copy produced by `navigator forms re-author
//! nv__business_trust_formation`.

use std::collections::BTreeMap;

fn two_people() -> String {
    r#"[
        {"name": "Aries Client", "street": "1 Main St", "city": "Las Vegas",
         "state": "NV", "zip": "89101", "country": "USA"},
        {"name": "Libra Partner", "street": "2 Side St", "city": "Reno",
         "state": "NV", "zip": "89501", "country": "USA"}
    ]"#
    .to_string()
}

fn sample_answers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("entity__company.name".into(), "Neon Demo Trust".into()),
        (
            "person__registered_agent.name".into(),
            "Neon Law Services".into(),
        ),
        ("people__trustees".into(), two_people()),
    ])
}

#[test]
#[ignore = "requires BUSINESS_TRUST_REAUTHORED_PDF=/path/to/re-authored/nv__business_trust_formation.pdf"]
fn real_business_trust_sample_answers_fill_reauthored_fields() {
    let path =
        std::env::var("BUSINESS_TRUST_REAUTHORED_PDF").expect("set BUSINESS_TRUST_REAUTHORED_PDF");
    let blank = std::fs::read(path).expect("read re-authored business-trust blank");
    let values = forms::fill_values("nv__business_trust_formation", &sample_answers())
        .expect("resolve fill values")
        .expect("form has fillable fields");

    assert_eq!(
        values.get("entity__company.name").map(String::as_str),
        Some("Neon Demo Trust")
    );
    assert_eq!(
        values.get("people__trustees.0.name").map(String::as_str),
        Some("Aries Client")
    );
    assert_eq!(
        values.get("people__trustees.0.title").map(String::as_str),
        Some("Trustee")
    );
    assert_eq!(
        values.get("people__trustees.1.name").map(String::as_str),
        Some("Libra Partner")
    );
    assert!(values.keys().all(|k| !k.starts_with("unmapped__")));

    let filled = pdf::fill_acroform(&blank, &values).expect("fill real business-trust blank");
    let read_back = pdf::read_field_values(&filled).expect("read filled values");
    for (field, expected) in values {
        assert_eq!(
            read_back.get(&field).map(String::as_str),
            Some(expected.as_str()),
            "{field} filled"
        );
    }
    if let Ok(path) = std::env::var("BUSINESS_TRUST_FLATTENED_OUTPUT") {
        let flattened = pdf::flatten(&filled).expect("flatten real business-trust sample");
        assert!(
            pdf::field_names(&flattened)
                .expect("flattened field names")
                .is_empty(),
            "flattened business-trust sample must expose no interactive fields"
        );
        assert_eq!(
            pdf::widget_annotation_count(&flattened).expect("flattened widget count"),
            0,
            "flattened business-trust sample must expose no widget annotations"
        );
        std::fs::write(path, flattened).expect("write flattened QA packet");
    }
}
