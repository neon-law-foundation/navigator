use std::collections::BTreeMap;

fn sample_answers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("person__client.name".into(), "Maria Santos".into()),
        ("person__client.family".into(), "Santos Gómez".into()),
        ("person__client.given".into(), "María".into()),
        ("person__client.middle".into(), "Elena".into()),
        ("person__client.email".into(), "maria@example.com".into()),
        ("custom_datetime__date_of_birth".into(), "1990-04-12".into()),
        ("country__of_birth.name".into(), "Mexico".into()),
        ("country__of_citizenship.name".into(), "Mexico".into()),
        ("custom_datetime__lpr_since".into(), "2019-03-01".into()),
        ("custom_phone__daytime_phone".into(), "702-555-0100".into()),
        (
            "custom_single_choice__eligibility_basis".into(),
            "five_year".into(),
        ),
        (
            "custom_single_choice__marital_status".into(),
            "married".into(),
        ),
        ("custom_text__time_outside_us".into(), "45".into()),
        ("custom_yes_no__good_moral_character".into(), "no".into()),
    ])
}

#[test]
#[ignore = "requires N400_REAUTHORED_PDF=/path/to/re-authored/us__naturalization.pdf"]
fn real_n400_sample_answers_fill_reauthored_fields() {
    let path = std::env::var("N400_REAUTHORED_PDF").expect("set N400_REAUTHORED_PDF");
    let blank = std::fs::read(path).expect("read re-authored N-400");
    let values = forms::fill_values("us__naturalization", &sample_answers())
        .expect("resolve fill values")
        .expect("form has fillable fields");

    // 8 first-slice states + the 3 structured legal-name parts (#311).
    assert_eq!(values.len(), 11);
    // The single display name is not a form field — the N-400 splits it.
    assert!(!values.contains_key("person__client.name"));
    assert_eq!(
        values.get("person__client.family").map(String::as_str),
        Some("Santos Gómez")
    );
    assert_eq!(
        values.get("person__client.given").map(String::as_str),
        Some("María")
    );
    assert_eq!(
        values.get("person__client.middle").map(String::as_str),
        Some("Elena")
    );
    // Absences and moral character stay deferred to a lawyer (#311).
    assert!(!values.contains_key("custom_text__time_outside_us"));
    assert!(!values.contains_key("custom_yes_no__good_moral_character"));
    assert_eq!(
        values
            .get("custom_single_choice__eligibility_basis")
            .map(String::as_str),
        Some("five_year")
    );
    assert_eq!(
        values
            .get("custom_single_choice__marital_status")
            .map(String::as_str),
        Some("married")
    );
    let filled = pdf::fill_acroform(&blank, &values).expect("fill real N-400");
    let read_back = pdf::read_field_values(&filled).expect("read filled values");
    for (field, expected) in values {
        assert_eq!(
            read_back.get(&field).map(String::as_str),
            Some(expected.as_str()),
            "{field} filled"
        );
    }

    // Stage the flattened artifact for visual QA (#305 standing rule):
    // confirm the legal-name parts land in Part 2 Line 1 and the Part 11
    // certification block.
    if let Ok(dir) = std::env::var("N400_QA_OUT") {
        let flat = pdf::flatten(&filled).expect("flatten filled N-400");
        std::fs::write(format!("{dir}/us__naturalization-name-qa.pdf"), &flat)
            .expect("write QA artifact");
    }
}
