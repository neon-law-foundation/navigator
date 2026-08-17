//! Cucumber runner for `features/source_code_preservation_tro.feature`.
//!
//! Composition lock for the source-code-preservation TRO notation. Unlike
//! `legal_workflow_shapes.rs` and `compliance_filings_workflow_shapes.rs`,
//! which load a template's markdown frontmatter, this suite drives the
//! *bundled standalone YAML* — the artifact the runtime actually reads.
//! `workflows/tests/spec_coherence.rs` separately proves the YAML and the
//! template frontmatter agree, so pinning the chain here pins both.

#![allow(clippy::unused_async)]

use cucumber::{gherkin::Step, given, then, World};
use features::template_shapes::walk_chain;
use workflows::{
    bundled_spec_yaml, questionnaire_spec_from_yaml, workflow_spec_from_yaml, WorkflowSpec,
};

#[derive(Default, World)]
#[world(init = Self::default)]
struct SpecWorld {
    yaml: Option<&'static str>,
}

impl std::fmt::Debug for SpecWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpecWorld")
            .field("has_yaml", &self.yaml.is_some())
            .finish()
    }
}

#[given(regex = r#"^the bundled spec yaml "([^"]+)"$"#)]
async fn load_bundled_spec(world: &mut SpecWorld, code: String) {
    world.yaml = Some(
        bundled_spec_yaml(&code)
            .unwrap_or_else(|| panic!("`{code}` is not registered in BUNDLED_SPEC_YAML")),
    );
}

#[then("the questionnaire transitions, in BEGIN-first order, are:")]
async fn assert_questionnaire_chain(world: &mut SpecWorld, step: &Step) {
    let yaml = world.yaml.expect("spec yaml loaded");
    let q = questionnaire_spec_from_yaml(yaml).expect("questionnaire yaml parses");
    assert_chain_matches(q.inner(), step);
}

#[then("the workflow transitions, in BEGIN-first order, are:")]
async fn assert_workflow_chain(world: &mut SpecWorld, step: &Step) {
    let yaml = world.yaml.expect("spec yaml loaded");
    let w = workflow_spec_from_yaml(yaml).expect("workflow yaml parses");
    assert_chain_matches(&w, step);
}

fn assert_chain_matches(spec: &WorkflowSpec, step: &Step) {
    let table = step.table.as_ref().expect("scenario has a data table");
    let expected: Vec<(&str, &str)> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| {
            (
                row.first().expect("from cell").as_str(),
                row.get(1).expect("to cell").as_str(),
            )
        })
        .collect();
    let chain = walk_chain(spec);
    let actual: Vec<(&str, &str)> = chain
        .iter()
        .map(|(f, t)| (f.as_str(), t.as_str()))
        .collect();
    assert_eq!(actual, expected, "transition chain mismatch");
}

#[tokio::main]
async fn main() {
    SpecWorld::cucumber()
        .run_and_exit("tests/features/source_code_preservation_tro.feature")
        .await;
}
