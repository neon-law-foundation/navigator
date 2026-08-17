//! Grounding test: `rules::BANK_PROMPTS` (the static bank-prompt table the
//! LSP shows on hover, kept free of the `store` dependency so the
//! LSP stays lean) must match the seeded `questions.prompt` wording in
//! `store/seeds/Question.yaml`. A prompt reworded in the seed without
//! updating the mirror (or vice-versa) fails here — so an author never hovers
//! a bank-backed state and sees stale wording.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
struct QuestionSeed {
    records: Vec<QuestionRecord>,
}

#[derive(Deserialize)]
struct QuestionRecord {
    code: String,
    prompt: String,
}

fn seed_prompts() -> BTreeMap<String, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate lives one level below the workspace root")
        .join("store/seeds/Question.yaml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let seed: QuestionSeed = serde_yaml::from_str(&text).expect("store/seeds/Question.yaml parses");
    seed.records
        .into_iter()
        .map(|r| (r.code, r.prompt))
        .collect()
}

#[test]
fn rules_bank_prompts_match_the_question_seed() {
    let seed = seed_prompts();
    let mirror: BTreeMap<String, String> = rules::BANK_PROMPTS
        .iter()
        .map(|(c, p)| ((*c).to_string(), (*p).to_string()))
        .collect();
    assert_eq!(
        mirror, seed,
        "rules::BANK_PROMPTS drifted from store/seeds/Question.yaml"
    );
}

#[test]
fn every_registered_type_has_a_bank_prompt() {
    for ty in rules::REGISTERED_QUESTION_TYPES {
        assert!(
            rules::bank_prompt(ty).is_some(),
            "registered type `{ty}` has no BANK_PROMPTS entry",
        );
    }
}
