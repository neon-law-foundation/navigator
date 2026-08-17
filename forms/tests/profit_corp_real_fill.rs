//! Fill the *real* re-authored NV Profit Corporation blank and assert
//! every resolved value round-trips through the actual bytes (#256 slice
//! 2). Ignored by default — the canonical blank lives only in the assets
//! bucket, so point the test at a working copy produced by `navigator
//! forms re-author nv__profit_corp_formation`. The offline
//! `fill_round_trip` suite covers the synthetic blank on every run; this
//! is the real-bytes proof that the `/T` names the transform minted match
//! what the resolver fills.

use std::collections::BTreeMap;

fn two_people() -> String {
    r#"[
        {"name": "Aries Client", "street": "1 Main St", "city": "Las Vegas",
         "state": "NV", "zip": "89101", "country": "USA", "title": "President"},
        {"name": "Libra Partner", "street": "2 Side St", "city": "Reno",
         "state": "NV", "zip": "89501", "country": "USA", "title": "Secretary"}
    ]"#
    .to_string()
}

fn sample_answers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("entity__company.name".into(), "Neon Demo Corp".into()),
        (
            "person__registered_agent.name".into(),
            "Neon Law Services".into(),
        ),
        ("custom_text__shares_authorized".into(), "1000".into()),
        ("custom_text__par_value".into(), "0.01".into()),
        ("people__directors".into(), two_people()),
        ("people__corporate_officers".into(), two_people()),
    ])
}

#[test]
#[ignore = "requires PROFIT_CORP_REAUTHORED_PDF=/path/to/re-authored/nv__profit_corp_formation.pdf"]
fn real_profit_corp_sample_answers_fill_reauthored_fields() {
    let path = std::env::var("PROFIT_CORP_REAUTHORED_PDF").expect("set PROFIT_CORP_REAUTHORED_PDF");
    let blank = std::fs::read(path).expect("read re-authored profit-corp blank");
    let values = forms::fill_values("nv__profit_corp_formation", &sample_answers())
        .expect("resolve fill values")
        .expect("form has fillable fields");

    // The scalar answers and both people-list rows resolve onto their
    // canonical `/T` paths; directors carry no title part, officers do.
    assert_eq!(
        values.get("entity__company.name").map(String::as_str),
        Some("Neon Demo Corp")
    );
    assert_eq!(
        values
            .get("custom_text__shares_authorized")
            .map(String::as_str),
        Some("1000")
    );
    assert_eq!(
        values.get("people__directors.0.name").map(String::as_str),
        Some("Aries Client")
    );
    assert_eq!(
        values
            .get("people__corporate_officers.0.title")
            .map(String::as_str),
        Some("President")
    );
    // Directors have no title row on the packet — the map never mapped a
    // director title, so none is minted.
    assert!(!values.contains_key("people__directors.0.title"));
    // The payment / COI / benefit-corp pages stay in the reserved
    // namespace and never fill.
    assert!(values.keys().all(|k| !k.starts_with("unmapped__")));

    // Every resolved value must land in — and read back from — the real
    // re-authored bytes, including the multi-widget merges (director 0 is
    // restated in the incorporator block under one `/T` name).
    let filled = pdf::fill_acroform(&blank, &values).expect("fill real profit-corp");
    let read_back = pdf::read_field_values(&filled).expect("read filled values");
    for (field, expected) in values {
        assert_eq!(
            read_back.get(&field).map(String::as_str),
            Some(expected.as_str()),
            "{field} filled"
        );
    }
}
