//! Verify and regenerate the repository-pinned `SendGrid` Mail API adapter.
//!
//! Ordinary builds never download a vendor schema or run a network-backed
//! generator. The explicit regeneration command renders the checked-in
//! adapter template from the pinned `OpenAPI` operation and writes the result.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

const SPEC_PATH: &str = "vendor/sendgrid/tsg_mail_v3.json";
const EXPECTED_SHA256: &str = "4ba24d0feb8a6b347f60528161d1cf0e7591987d2b3a64a6533685dc2b39d19f";
const ADAPTER_PATH: &str = "workflows/src/email/sendgrid_openapi.rs";
const TEMPLATE_PATH: &str = "vendor/sendgrid/sendgrid_openapi.rs.template";
const EXPECTED_ADAPTER_SHA256: &str =
    "2a38f58662025087e42e5eb043fda275e7f80e513a614cea56fd20e58dd90f18";
const GENERATOR_VERSION: &str = "navigator-openapi-rust-1";

struct SendMailContract {
    operation_path: String,
    required_fields: Vec<String>,
}

fn sha256(bytes: &[u8]) -> String {
    let mut actual = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut actual, "{byte:02x}").expect("writing to a String cannot fail");
    }
    actual
}

fn read_contract(root: &Path) -> Result<(Vec<u8>, SendMailContract)> {
    let path = root.join(SPEC_PATH);
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;

    let paths = value
        .pointer("/paths")
        .and_then(serde_json::Value::as_object)
        .context("SendGrid OpenAPI contract has no paths object")?;
    let mut operations = paths.iter().filter_map(|(path, item)| {
        let operation = item.get("post")?;
        (operation
            .get("operationId")
            .and_then(serde_json::Value::as_str)
            == Some("SendMail"))
        .then_some((path, operation))
    });
    let (operation_path, operation) = operations
        .next()
        .context("SendGrid OpenAPI contract must expose a SendMail POST operation")?;
    if operations.next().is_some() {
        bail!("SendGrid OpenAPI contract exposes SendMail more than once");
    }

    let request_body = operation
        .pointer("/requestBody/$ref")
        .and_then(serde_json::Value::as_str)
        .and_then(|reference| reference.strip_prefix("#"))
        .and_then(|pointer| value.pointer(pointer))
        .or_else(|| operation.pointer("/requestBody"))
        .context("SendGrid SendMail operation has no request body")?;
    let schema = request_body
        .pointer("/content/application~1json/schema")
        .context("SendGrid SendMail operation has no JSON request schema")?;
    let schema = schema
        .get("$ref")
        .and_then(serde_json::Value::as_str)
        .and_then(|reference| reference.strip_prefix("#"))
        .and_then(|pointer| value.pointer(pointer))
        .unwrap_or(schema);
    let required = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .context("SendGrid SendMail request schema has no required fields")?;
    let mut required_fields = BTreeSet::new();
    for field in required {
        let field = field
            .as_str()
            .context("SendGrid SendMail required fields must be strings")?;
        required_fields.insert(field.to_owned());
    }

    Ok((
        bytes,
        SendMailContract {
            operation_path: operation_path.to_owned(),
            required_fields: required_fields.into_iter().collect(),
        },
    ))
}

fn render_adapter(root: &Path) -> Result<Vec<u8>> {
    let (spec_bytes, contract) = read_contract(root)?;
    let template_path = root.join(TEMPLATE_PATH);
    let template = std::fs::read_to_string(&template_path)
        .with_context(|| format!("read adapter template {}", template_path.display()))?;
    let required_fields = contract
        .required_fields
        .iter()
        .map(|field| serde_json::to_string(field).expect("field names are strings"))
        .collect::<Vec<_>>()
        .join(", ");
    let rendered = template
        .replace("{{SPEC_SHA256}}", &sha256(&spec_bytes))
        .replace("{{OPERATION_PATH}}", &contract.operation_path)
        .replace("{{REQUIRED_FIELDS}}", &required_fields);
    if rendered.contains("{{") || rendered.contains("}}") {
        bail!("SendGrid adapter template contains an unreplaced placeholder");
    }
    Ok(rendered.into_bytes())
}

fn pin_update_instructions(actual_spec_sha: &str, actual_adapter_sha: &str) -> String {
    format!(
        "SendGrid pin update required; the adapter was not written.\n\
         Update cli/src/sendgrid_openapi.rs:\n\
           const EXPECTED_SHA256: &str = \"{actual_spec_sha}\";\n\
           const EXPECTED_ADAPTER_SHA256: &str = \"{actual_adapter_sha}\";\n\
         Then rerun:\n\
           cargo run -p cli -- dev sendgrid-openapi --regenerate"
    )
}

pub fn verify(root: &Path) -> Result<()> {
    let (spec_bytes, _) = read_contract(root)?;
    let actual_spec_sha = sha256(&spec_bytes);
    if actual_spec_sha != EXPECTED_SHA256 {
        bail!(
            "SendGrid OpenAPI drift: {SPEC_PATH} has SHA-256 {actual_spec_sha}, expected {EXPECTED_SHA256}"
        );
    }

    let adapter = root.join(ADAPTER_PATH);
    let adapter_bytes = std::fs::read(&adapter)
        .with_context(|| format!("read generated adapter {}", adapter.display()))?;
    let actual_adapter_sha = sha256(&adapter_bytes);
    if actual_adapter_sha != EXPECTED_ADAPTER_SHA256 {
        bail!(
            "SendGrid generated adapter drift: {ADAPTER_PATH} has SHA-256 {actual_adapter_sha}, expected {EXPECTED_ADAPTER_SHA256}"
        );
    }
    let rendered = render_adapter(root)?;
    if adapter_bytes != rendered {
        bail!(
            "SendGrid generated adapter drift: {ADAPTER_PATH} does not match the deterministic output of {TEMPLATE_PATH}"
        );
    }
    println!(
        "SendGrid OpenAPI verified: {SPEC_PATH} ({EXPECTED_SHA256}), generator {GENERATOR_VERSION}"
    );
    Ok(())
}

/// Render the adapter from the pinned specification and write it in place.
pub fn regenerate(root: &Path) -> Result<()> {
    let (spec_bytes, _) = read_contract(root)?;
    let actual_spec_sha = sha256(&spec_bytes);
    let rendered = render_adapter(root)?;
    let actual_adapter_sha = sha256(&rendered);
    if actual_spec_sha != EXPECTED_SHA256 || actual_adapter_sha != EXPECTED_ADAPTER_SHA256 {
        bail!(
            "{}",
            pin_update_instructions(&actual_spec_sha, &actual_adapter_sha)
        );
    }

    let adapter = root.join(ADAPTER_PATH);
    if adapter.parent().is_some_and(|parent| !parent.exists()) {
        std::fs::create_dir_all(adapter.parent().expect("checked above"))
            .with_context(|| format!("create generated adapter directory {}", adapter.display()))?;
    }
    let changed = std::fs::read(&adapter).map_or(true, |current| current != rendered);
    if changed {
        std::fs::write(&adapter, &rendered)
            .with_context(|| format!("write generated adapter {}", adapter.display()))?;
    }
    verify(root)?;
    println!(
        "SendGrid adapter {}: {}",
        if changed {
            "regenerated"
        } else {
            "already current"
        },
        adapter.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn regenerate_restores_adapter_from_the_pinned_contract() {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("cli is nested under the workspace")
            .to_path_buf();
        let temp = tempfile::tempdir().expect("create temporary workspace");
        let root = temp.path();
        std::fs::create_dir_all(root.join("vendor/sendgrid")).expect("create vendor directory");
        std::fs::copy(workspace.join(SPEC_PATH), root.join(SPEC_PATH))
            .expect("copy pinned contract");
        std::fs::copy(workspace.join(TEMPLATE_PATH), root.join(TEMPLATE_PATH))
            .expect("copy adapter template");
        let adapter = root.join(ADAPTER_PATH);
        std::fs::create_dir_all(adapter.parent().expect("adapter parent"))
            .expect("create adapter directory");
        std::fs::write(&adapter, b"stale adapter").expect("write stale adapter");

        regenerate(root).expect("regenerate adapter");
        verify(root).expect("verify regenerated adapter");
        assert_eq!(
            std::fs::read(adapter).expect("read regenerated adapter"),
            std::fs::read(workspace.join(ADAPTER_PATH)).expect("read checked-in adapter")
        );
    }

    #[test]
    fn regenerate_reports_unpinned_hashes_without_writing_the_adapter() {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("cli is nested under the workspace")
            .to_path_buf();
        let temp = tempfile::tempdir().expect("create temporary workspace");
        let root = temp.path();
        std::fs::create_dir_all(root.join("vendor/sendgrid")).expect("create vendor directory");
        std::fs::copy(workspace.join(SPEC_PATH), root.join(SPEC_PATH))
            .expect("copy pinned contract");
        std::fs::copy(workspace.join(TEMPLATE_PATH), root.join(TEMPLATE_PATH))
            .expect("copy adapter template");
        let spec = root.join(SPEC_PATH);
        std::fs::write(
            &spec,
            [
                std::fs::read(&spec).expect("read copied contract"),
                b"\n".to_vec(),
            ]
            .concat(),
        )
        .expect("make an intentional pin bump");
        let adapter = root.join(ADAPTER_PATH);
        std::fs::create_dir_all(adapter.parent().expect("adapter parent"))
            .expect("create adapter directory");
        std::fs::write(&adapter, b"adapter must remain untouched").expect("write sentinel adapter");

        let expected_spec_sha = sha256(&std::fs::read(&spec).expect("read bumped contract"));
        let expected_adapter_sha = sha256(&render_adapter(root).expect("render bumped adapter"));
        let error = regenerate(root).expect_err("unpinned inputs must fail before writing");

        assert_eq!(
            std::fs::read(&adapter).expect("read sentinel adapter"),
            b"adapter must remain untouched"
        );
        let message = error.to_string();
        assert!(message.contains(&format!(
            "const EXPECTED_SHA256: &str = \"{expected_spec_sha}\";"
        )));
        assert!(message.contains(&format!(
            "const EXPECTED_ADAPTER_SHA256: &str = \"{expected_adapter_sha}\";"
        )));
        assert!(message.contains("cargo run -p cli -- dev sendgrid-openapi --regenerate"));
    }
}
