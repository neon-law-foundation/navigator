//! Grounding test: the `rules` crate's `REGISTERED_QUESTION_TYPES` (the
//! lightweight vocabulary `N113` checks against, kept free of the
//! `store`/`sea_orm` dependency so the LSP stays lean) must be exactly the
//! set of `store::question_registry::QuestionType` tokens. `cli` is the
//! narrowest crate that depends on both, so the parity is pinned here — a
//! new registry variant that forgets the `rules` mirror (or vice-versa)
//! fails this test.

#[test]
fn rules_registered_types_match_the_store_registry() {
    let mut from_store = store::question_registry::QuestionType::all_tokens();
    from_store.sort_unstable();
    let mut from_rules = rules::REGISTERED_QUESTION_TYPES.to_vec();
    from_rules.sort_unstable();
    assert_eq!(
        from_rules, from_store,
        "rules::REGISTERED_QUESTION_TYPES drifted from store::question_registry::QuestionType"
    );
}
