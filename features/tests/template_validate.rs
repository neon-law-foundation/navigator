//! Cucumber runner for `features/template_validate.feature`.
//!
//! Exercises the pure `rules::Rule::lint` surface — no DB, no async,
//! just source file in, violations out.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use std::path::PathBuf;

use cucumber::{gherkin::Step, given, then, when, World};
use rules::{
    F101FrontmatterTitle, F102RespondentType, Rule, S101LineLength, SourceFile, Violation,
};

#[derive(Debug, Default, World)]
struct ValidateWorld {
    source: String,
    violations: Vec<Violation>,
}

fn rule_by_code(code: &str) -> Box<dyn Rule> {
    match code {
        S101LineLength::CODE => Box::new(S101LineLength::default()),
        F101FrontmatterTitle::CODE => Box::new(F101FrontmatterTitle),
        F102RespondentType::CODE => Box::new(F102RespondentType),
        other => panic!("unknown rule code in feature file: {other}"),
    }
}

#[given("the markdown:")]
async fn capture_markdown(world: &mut ValidateWorld, step: &Step) {
    world.source = step
        .docstring
        .as_ref()
        .expect("Given block needs a docstring")
        .trim_start_matches('\n')
        .to_string();
}

#[when(regex = r#"^the markdown is linted with rule "([^"]+)"$"#)]
async fn lint(world: &mut ValidateWorld, code: String) {
    let rule = rule_by_code(&code);
    let file = SourceFile {
        path: PathBuf::from("scenario.md"),
        contents: world.source.clone(),
    };
    world.violations = rule.lint(&file);
}

#[then(regex = r"^(\d+) violations? (?:is|are) reported$")]
async fn assert_count(world: &mut ValidateWorld, expected: usize) {
    assert_eq!(
        world.violations.len(),
        expected,
        "violations: {:?}",
        world.violations,
    );
}

#[then(regex = r#"^the violation code is "([^"]+)"$"#)]
async fn assert_code(world: &mut ValidateWorld, expected: String) {
    let actual = world.violations.first().expect("at least one violation");
    assert_eq!(actual.code, expected);
}

#[then(regex = r#"^the violation message contains "([^"]+)"$"#)]
async fn assert_message(world: &mut ValidateWorld, needle: String) {
    let actual = world.violations.first().expect("at least one violation");
    assert!(
        actual.message.contains(&needle),
        "message {:?} does not contain {needle:?}",
        actual.message,
    );
}

#[tokio::main]
async fn main() {
    ValidateWorld::cucumber()
        .run("tests/features/template_validate.feature")
        .await;
}
