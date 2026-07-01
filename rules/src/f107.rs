//! `N107` — notation template signature placeholders must resolve.
//!
//! A *signature placeholder* is a `{{ … }}` token whose trimmed inner
//! text names a signer and field — e.g. `{{client.signature}}`. Dotted
//! data paths such as `{{person__client.name}}` and aggregate loop
//! variables such as `{{m.name}}` belong to `N115`, so this rule skips
//! questionnaire states and lexical `#for` variables.
//!
//! The placeholder splits on the **first** dot into `<signer>.<field>`:
//!
//! - `signer` must be one of [`F107SignaturePlaceholders::SIGNERS`]
//!   (`client`, `firm`). Roles, never a person's name — the signer
//!   resolves to a real Person (the respondent, or the attorney of
//!   record) at notation time.
//! - `field` must be one of [`F107SignaturePlaceholders::FIELDS`]
//!   (`signature`, `initials`, `date`).
//!
//! The signing declaration is **bidirectional** — a well-formed
//! signature is declared in two places that must agree:
//!
//! - **Forward:** a template that draws *any* valid signature block must
//!   declare a signing State in its workflow — a state keyed
//!   `sent_for_signature` or prefixed `sent_for_signature__`. A
//!   signature line that never becomes an attributable signature is a
//!   candor problem.
//! - **Reverse:** a template whose workflow declares a
//!   `sent_for_signature[__*]` State must carry at least one body
//!   signature anchor for the field to land on. A signing step with
//!   nowhere to sign is the same candor gap in mirror image — and it is
//!   exactly how a template reaches the e-signature provider with a
//!   signing state but no placed tab (the live retainer bug).
//!
//! Both directions are deliberate: the provider's signers and tabs are
//! derived from this two-part declaration, never from ad-hoc UI
//! placement, so the template can never reach the provider with one half
//! of the pair missing.
//!
//! Files without frontmatter are skipped: N107 is a notation-template
//! rule, not a check on arbitrary prose that happens to contain a
//! `{{x.y}}` token.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{frontmatter, Rule, SourceFile, Violation};

const FOR_OPEN: &str = "{{#for ";

pub struct F107SignaturePlaceholders;

impl F107SignaturePlaceholders {
    pub const CODE: &'static str = "N107";

    /// Recognized signer roles. Roles, never names — extend this list
    /// when a new signer (e.g. `witness`, `notary`) enters the bench.
    pub const SIGNERS: &'static [&'static str] = &["client", "firm"];

    /// Recognized signature field types, each mapping to a distinct
    /// downstream e-signature tab (signHere / initialHere / dateSigned).
    pub const FIELDS: &'static [&'static str] = &["signature", "initials", "date"];

    /// The workflow State prefix that marks a signing step.
    const SIGNING_STATE: &'static str = "sent_for_signature";
}

/// One `{{ signer.field }}` token found in a source file. `offset` is
/// the byte offset of the opening `{{`; `end` is the byte offset just
/// past the closing `}}` (so `contents[offset..end]` is the whole
/// token); `signer`/`field` are the halves of the trimmed inner text
/// split on the first `.`.
///
/// Exposed so the renderer and the signature-manifest builder consume
/// the *same* grammar this rule validates — one parser, one source of
/// truth for what a signature placeholder is. `offset`/`end` let the
/// renderer splice the token out and replace it without re-scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaturePlaceholder {
    pub offset: usize,
    pub end: usize,
    pub signer: String,
    pub field: String,
}

/// Scan `contents` for signature placeholders: every `{{ … }}` token
/// whose trimmed inner text contains a `.`. Data placeholders without
/// a dot are ignored. The split is on the *first* dot, so `{{a.b.c}}`
/// yields `signer = "a"`, `field = "b.c"` (which the rule then flags
/// as an unknown field).
#[must_use]
pub fn signature_placeholders(contents: &str) -> Vec<SignaturePlaceholder> {
    let bytes = contents.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(rel) = contents[i + 2..].find("}}") {
                let end = i + 2 + rel + 2;
                let inner = contents[i + 2..i + 2 + rel].trim();
                if let Some((signer, field)) = inner.split_once('.') {
                    out.push(SignaturePlaceholder {
                        offset: i,
                        end,
                        signer: signer.trim().to_string(),
                        field: field.trim().to_string(),
                    });
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Lexical variables introduced by `{{#for <var> in <state>}}` blocks.
fn loop_variables(contents: &str) -> std::collections::BTreeSet<String> {
    let mut vars = std::collections::BTreeSet::new();
    let mut rest = contents;
    while let Some(start) = rest.find(FOR_OPEN) {
        let after_open = &rest[start + FOR_OPEN.len()..];
        let Some(header_len) = after_open.find("}}") else {
            break;
        };
        let header = after_open[..header_len].trim();
        if let Some((var, _state)) = header.split_once(" in ") {
            vars.insert(var.trim().to_string());
        }
        rest = &after_open[header_len + 2..];
    }
    vars
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    workflow: Option<BTreeMap<String, BTreeMap<String, String>>>,
    #[serde(default)]
    questionnaire: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

/// 1-based line number containing the byte at `offset`.
fn line_at(contents: &str, offset: usize) -> usize {
    contents[..offset.min(contents.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

impl Rule for F107SignaturePlaceholders {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        // Notation-template rule: skip files without frontmatter.
        let Some(fm) = frontmatter::extract(&file.contents) else {
            return Vec::new();
        };

        let placeholders = signature_placeholders(&file.contents);
        let parsed = serde_yaml::from_str::<FrontmatterShape>(fm).ok();
        let questionnaire_states: std::collections::BTreeSet<&str> = parsed
            .as_ref()
            .and_then(|p| p.questionnaire.as_ref())
            .into_iter()
            .flat_map(|q| q.keys().map(String::as_str))
            .collect();
        let loop_variables = loop_variables(&file.contents);
        let mut violations = Vec::new();
        let mut saw_valid_block = false;
        // A *relevant* anchor is any placeholder that survives the
        // data-grammar filter above — a candidate signature token, valid
        // (`{{client.signature}}`) or malformed (`{{spouse.signature}}`).
        // Loop row tokens (`{{m.name}}`) and questionnaire-state paths are
        // N115 data grammar, so they never count as a signature anchor.
        let mut saw_relevant_anchor = false;

        for ph in &placeholders {
            if questionnaire_states.contains(ph.signer.as_str())
                || loop_variables.contains(ph.signer.as_str())
            {
                continue;
            }
            saw_relevant_anchor = true;
            let signer_ok = Self::SIGNERS.contains(&ph.signer.as_str());
            let field_ok = Self::FIELDS.contains(&ph.field.as_str());
            let line = line_at(&file.contents, ph.offset);
            let range = ph.offset..ph.offset;

            if !signer_ok {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line,
                    range: range.clone(),
                    message: format!(
                        "unknown signer role `{}` in signature placeholder (expected one of: {})",
                        ph.signer,
                        Self::SIGNERS.join(", ")
                    ),
                });
            }
            if !field_ok {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line,
                    range,
                    message: format!(
                        "unknown signature field `{}` in signature placeholder (expected one of: {})",
                        ph.field,
                        Self::FIELDS.join(", ")
                    ),
                });
            }
            if signer_ok && field_ok {
                saw_valid_block = true;
            }
        }

        // The signing declaration is bidirectional: a body signature
        // block and a `sent_for_signature[__*]` State must each imply the
        // other. Parse the workflow once and cross-check both directions.
        let has_signing_state = parsed.and_then(|p| p.workflow).is_some_and(|wf| {
            wf.keys().any(|state| {
                state == Self::SIGNING_STATE
                    || state.starts_with(&format!("{}__", Self::SIGNING_STATE))
            })
        });

        // Forward: a signature block must have somewhere to collect the
        // signature.
        if saw_valid_block && !has_signing_state {
            violations.push(Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: 0..0,
                message: format!(
                    "template draws a signature block but its workflow has no \
                     `{}` (or `{}__*`) state to collect the signature",
                    Self::SIGNING_STATE,
                    Self::SIGNING_STATE
                ),
            });
        }

        // Reverse: a signing State must have a body anchor for its tab to
        // land on — otherwise the provider gets a signing step with no
        // placed signature field (the live retainer bug). We key off "no
        // *relevant* signature token", not "no valid one": a malformed
        // token (`{{spouse.signature}}`) already draws its own
        // unknown-signer violation, so adding "no anchor" on top would be
        // contradictory noise. Keying off `placeholders.is_empty()` alone
        // would let a signing template that uses a `{{#for}}` loop but
        // forgets its real signature block slip through — the loop row
        // tokens are data grammar, not anchors, so they must not satisfy
        // this check.
        if has_signing_state && !saw_relevant_anchor {
            violations.push(Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: 1,
                range: 0..0,
                message: format!(
                    "workflow declares a `{}` state but the body carries no signature \
                     anchor (expected at least one `{{{{<signer>.<field>}}}}`, e.g. \
                     `{{{{client.signature}}}}`) for the tab to land on",
                    Self::SIGNING_STATE
                ),
            });
        }

        violations
    }
}

#[cfg(test)]
mod tests {
    use super::{signature_placeholders, F107SignaturePlaceholders};
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("retainer.md"),
            contents: body.to_string(),
        }
    }

    /// A frontmatter block with a valid signing workflow, plus whatever
    /// body the test appends.
    fn with_signing_workflow(body: &str) -> String {
        format!(
            "---\ntitle: Retainer\nworkflow:\n  BEGIN:\n    created: sent_for_signature__pending\n  \
             sent_for_signature__pending:\n    signature_received: END\n  END: {{}}\n---\n{body}"
        )
    }

    #[test]
    fn parser_ignores_data_placeholders_without_a_dot() {
        let found = signature_placeholders("Hello {{client_name}} and {{project_name}}.");
        assert!(found.is_empty(), "data placeholders have no dot: {found:?}");
    }

    #[test]
    fn parser_extracts_signer_and_field_split_on_first_dot() {
        let found = signature_placeholders("{{client.signature}} {{firm.date}}");
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].signer.as_str(), found[0].field.as_str()),
            ("client", "signature")
        );
        assert_eq!(
            (found[1].signer.as_str(), found[1].field.as_str()),
            ("firm", "date")
        );
    }

    #[test]
    fn parser_offset_and_end_span_the_whole_token() {
        let body = "x {{client.signature}} y";
        let found = signature_placeholders(body);
        assert_eq!(found.len(), 1);
        assert_eq!(&body[found[0].offset..found[0].end], "{{client.signature}}");
    }

    #[test]
    fn parser_trims_inner_whitespace() {
        let found = signature_placeholders("{{  client.signature  }}");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].signer, "client");
        assert_eq!(found[0].field, "signature");
    }

    #[test]
    fn loop_variable_paths_are_not_signature_placeholders() {
        let body = "---\nquestionnaire:\n  BEGIN:\n    _: people__members\n  people__members:\n    _: END\n  END: {}\nworkflow:\n  BEGIN:\n    intake_submitted: staff_review\n  staff_review:\n    approved: END\n  END: {}\n---\n{{#for m in people__members}}{{m.name}} from {{m.city}}{{/for}}";
        assert!(
            F107SignaturePlaceholders.lint(&file(body)).is_empty(),
            "loop row fields are N115 data grammar, not N107 signature grammar"
        );
    }

    #[test]
    fn passes_valid_client_and_firm_blocks_with_signing_state() {
        let body = with_signing_workflow(
            "Signature: {{client.signature}} {{client.date}}\nCountersigned: {{firm.signature}}\n",
        );
        assert!(
            F107SignaturePlaceholders.lint(&file(&body)).is_empty(),
            "well-formed signature blocks with a signing state should pass",
        );
    }

    #[test]
    fn passes_initials_block() {
        let body = with_signing_workflow("Initials: {{client.initials}}\n");
        assert!(F107SignaturePlaceholders.lint(&file(&body)).is_empty());
    }

    #[test]
    fn no_frontmatter_means_no_violation() {
        // Arbitrary prose with a dotted token is NOT a template.
        let v = F107SignaturePlaceholders.lint(&file("see {{config.value}} in the docs"));
        assert!(v.is_empty());
    }

    #[test]
    fn flags_unknown_signer_role() {
        let body = with_signing_workflow("{{spouse.signature}}\n");
        let v = F107SignaturePlaceholders.lint(&file(&body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].code, "N107");
        assert!(v[0].message.contains("unknown signer role `spouse`"));
    }

    #[test]
    fn flags_unknown_field_type() {
        let body = with_signing_workflow("{{client.fingerprint}}\n");
        let v = F107SignaturePlaceholders.lint(&file(&body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0]
            .message
            .contains("unknown signature field `fingerprint`"));
    }

    #[test]
    fn rejects_person_name_signer() {
        // Role, never a name: `{{nick.signature}}` is not a valid block.
        let body = with_signing_workflow("{{nick.signature}}\n");
        let v = F107SignaturePlaceholders.lint(&file(&body));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("unknown signer role `nick`"));
    }

    #[test]
    fn flags_signature_block_without_signing_workflow_state() {
        // Valid grammar, but the workflow has no place to collect it.
        let body = "---\ntitle: Retainer\nworkflow:\n  BEGIN:\n    created: staff_review\n  \
                    staff_review:\n    approved: END\n  END: {}\n---\n{{client.signature}}\n";
        let v = F107SignaturePlaceholders.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("no `sent_for_signature`"));
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn flags_signing_state_without_a_body_anchor() {
        // Reverse direction: the workflow declares a signing state but
        // the body has no signature anchor for the tab to land on — the
        // mirror of the live retainer bug (signing step, no placed tab).
        let body = "---\ntitle: Retainer\nworkflow:\n  BEGIN:\n    \
                    created: sent_for_signature__pending\n  \
                    sent_for_signature__pending:\n    signature_received: END\n  \
                    END: {}\n---\nDear {{client_name}}, please sign offline.\n";
        let v = F107SignaturePlaceholders.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].code, "N107");
        assert!(
            v[0].message.contains("no signature anchor"),
            "message was: {}",
            v[0].message
        );
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn signing_state_with_a_malformed_anchor_does_not_double_flag_reverse() {
        // A present-but-malformed token draws its own unknown-signer
        // violation; the reverse "no anchor" check must stay silent so
        // the author gets one clear message, not a contradictory pair.
        let body = "---\ntitle: R\nworkflow:\n  BEGIN:\n    \
                    created: sent_for_signature\n  sent_for_signature:\n    \
                    signature_received: END\n  END: {}\n---\n{{spouse.signature}}\n";
        let v = F107SignaturePlaceholders.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("unknown signer role"));
        assert!(
            !v.iter().any(|x| x.message.contains("no signature anchor")),
            "reverse check must not fire when a (malformed) anchor is present",
        );
    }

    #[test]
    fn signing_state_with_only_loop_tokens_still_flags_missing_anchor() {
        // A signing workflow whose body uses a `{{#for}}` loop but forgets
        // its real signature block. The loop row tokens (`{{m.name}}`) are
        // N115 data grammar, not signature anchors, so the reverse check
        // must still fire — else `expand_signatures` places zero tabs and
        // the template reaches the provider with a signing step and nowhere
        // to sign (the very bug this guard exists to catch).
        let body = "---\ntitle: Retainer\nquestionnaire:\n  BEGIN:\n    _: people__members\n  \
                    people__members:\n    _: END\n  END: {}\nworkflow:\n  BEGIN:\n    \
                    created: sent_for_signature__pending\n  \
                    sent_for_signature__pending:\n    signature_received: END\n  \
                    END: {}\n---\nMembers:\n{{#for m in people__members}}- {{m.name}}\n{{/for}}\n";
        let v = F107SignaturePlaceholders.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].code, "N107");
        assert!(
            v[0].message.contains("no signature anchor"),
            "message was: {}",
            v[0].message
        );
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn bare_sent_for_signature_state_satisfies_cross_check() {
        let body = "---\ntitle: R\nworkflow:\n  BEGIN:\n    created: sent_for_signature\n  \
                    sent_for_signature:\n    signature_received: END\n  END: {}\n---\n\
                    {{client.signature}}\n";
        assert!(F107SignaturePlaceholders.lint(&file(body)).is_empty());
    }

    #[test]
    fn reports_violation_at_the_token_line() {
        let body = with_signing_workflow("line one\nline two has {{client.oops}}\n");
        let v = F107SignaturePlaceholders.lint(&file(&body));
        assert_eq!(v.len(), 1);
        // The frontmatter is 6 lines; body line "line one" follows, then
        // the offending token is on the next line. Assert it is NOT 1.
        assert!(
            v[0].line > 1,
            "violation should point at the token, not line 1"
        );
    }

    #[test]
    fn no_signature_blocks_means_workflow_cross_check_is_silent() {
        // A template with only data placeholders never triggers the
        // signing-state requirement.
        let body = "---\ntitle: T\nworkflow:\n  BEGIN:\n    created: END\n  END: {}\n---\n\
                    Dear {{client_name}}, welcome.\n";
        assert!(F107SignaturePlaceholders.lint(&file(body)).is_empty());
    }
}
