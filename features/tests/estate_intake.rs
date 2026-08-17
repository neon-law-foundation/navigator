//! Cucumber runner for `features/estate_intake.feature`.
//!
//! Pins the `onboarding__estate` (Northstar) workflow + questionnaire
//! shape directly against the bundled spec. The machine branches on
//! named conditions (attorney approve/reject, signature received/
//! declined), so — like the Nevada trust — it is route-checked here
//! rather than walked as a linear `_` chain. Complements
//! `workflows/tests/estate_intake_spec.rs`, which also drives the happy
//! path on the in-memory runtime.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use cucumber::{gherkin::Step, then, World};
use workflows::{
    bundled_spec_yaml, questionnaire_spec_from_yaml, step_kind_for, workflow_spec_from_yaml,
    StateName,
};

const TEMPLATE_CODE: &str = "onboarding__estate";

#[derive(Debug, Default, World)]
#[world(init = Self::default)]
struct EstateWorld;

#[then("the onboarding__estate workflow routes:")]
async fn assert_workflow_routes(_world: &mut EstateWorld, step: &Step) {
    let yaml = bundled_spec_yaml(TEMPLATE_CODE).expect("onboarding__estate has a bundled spec");
    let spec = workflow_spec_from_yaml(yaml).expect("estate workflow spec parses");
    let table = step.table.as_ref().expect("scenario has a data table");
    for row in table.rows.iter().skip(1) {
        let from = StateName::from(row.first().expect("from cell").as_str());
        let condition = row.get(1).expect("condition cell").as_str();
        let to = row.get(2).expect("to cell").as_str();
        let actual = spec
            .transitions_from(&from)
            .and_then(|t| t.lookup(condition))
            .unwrap_or_else(|| panic!("no `{condition}` transition out of `{}`", from.as_str()));
        assert_eq!(
            actual.as_str(),
            to,
            "`{}` --{condition}--> expected `{to}`",
            from.as_str()
        );
    }
}

#[then("the onboarding__estate questionnaire routes:")]
async fn assert_questionnaire_routes(_world: &mut EstateWorld, step: &Step) {
    let yaml = bundled_spec_yaml(TEMPLATE_CODE).expect("onboarding__estate has a bundled spec");
    let spec = questionnaire_spec_from_yaml(yaml).expect("estate questionnaire spec parses");
    let table = step.table.as_ref().expect("scenario has a data table");
    for row in table.rows.iter().skip(1) {
        let from = StateName::from(row.first().expect("from cell").as_str());
        let condition = row.get(1).expect("condition cell").as_str();
        let to = row.get(2).expect("to cell").as_str();
        let actual = spec
            .inner()
            .transitions_from(&from)
            .and_then(|t| t.lookup(condition))
            .unwrap_or_else(|| panic!("no `{condition}` transition out of `{}`", from.as_str()));
        assert_eq!(
            actual.as_str(),
            to,
            "`{}` --{condition}--> expected `{to}`",
            from.as_str()
        );
    }
}

#[then("every onboarding__estate workflow state resolves to a StepKind")]
async fn assert_step_kinds_resolve(_world: &mut EstateWorld) {
    let yaml = bundled_spec_yaml(TEMPLATE_CODE).expect("onboarding__estate has a bundled spec");
    let spec = workflow_spec_from_yaml(yaml).expect("estate workflow spec parses");
    for state in spec.states.keys() {
        if state.as_str() == StateName::END {
            continue;
        }
        assert!(
            step_kind_for(state).is_some(),
            "state `{}` has no StepKind (prefix `{}` is unrouted)",
            state.as_str(),
            state.prefix(),
        );
    }
}

#[tokio::main]
async fn main() {
    EstateWorld::cucumber()
        .run_and_exit("tests/features/estate_intake.feature")
        .await;
}
