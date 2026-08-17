//! `M052` — every reference-link label used in the document must
//! have a matching `[label]: dest` definition. Mirrors MD052.

use crate::{line_byte_range, Rule, SourceFile, Violation};
use std::collections::HashSet;

pub struct M052ReferenceLinksImages;

impl M052ReferenceLinksImages {
    pub const CODE: &'static str = "M052";
}

fn normalize(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut last_space = false;
    for ch in label.trim().chars().map(|c| c.to_ascii_lowercase()) {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

fn collect_definitions(contents: &str) -> HashSet<String> {
    let mut defs = HashSet::new();
    for line in contents.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix('[') {
            if let Some(end) = rest.find("]:") {
                let label = &rest[..end];
                defs.insert(normalize(label));
            }
        }
    }
    defs
}

/// Returns labels used as reference or collapsed-reference links, in
/// the form `[text][label]`, `[label][]`, or `![alt][label]`. Used by
/// `M053` to determine which definitions are referenced.
#[must_use]
pub fn used_labels_public(line: &str) -> Vec<String> {
    used_labels(line)
}

fn used_labels(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // Find matching `]`
            let mut j = i + 1;
            let mut depth = 1;
            while j < bytes.len() {
                match bytes[j] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    b'\\' if j + 1 < bytes.len() => j += 1,
                    _ => {}
                }
                j += 1;
            }
            if j >= bytes.len() {
                i += 1;
                continue;
            }
            let text = &line[i + 1..j];
            // Inline link: skip entirely.
            if bytes.get(j + 1) == Some(&b'(') {
                let mut k = j + 2;
                let mut d = 1;
                while k < bytes.len() && d > 0 {
                    match bytes[k] {
                        b'(' => d += 1,
                        b')' => d -= 1,
                        _ => {}
                    }
                    k += 1;
                }
                i = k;
                continue;
            }
            // Reference or collapsed-reference: `[text][label]` or `[label][]`.
            if bytes.get(j + 1) == Some(&b'[') {
                if let Some(close) = line[j + 2..].find(']') {
                    let label_raw = &line[j + 2..j + 2 + close];
                    let label = if label_raw.is_empty() {
                        text
                    } else {
                        label_raw
                    };
                    out.push(normalize(label));
                    i = j + 3 + close;
                    continue;
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn is_definition_line(line: &str) -> bool {
    let t = line.trim_start();
    if !t.starts_with('[') {
        return false;
    }
    t.find("]:").is_some()
}

impl Rule for M052ReferenceLinksImages {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let defs = collect_definitions(&file.contents);
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            if is_definition_line(line) {
                continue;
            }
            for label in used_labels(line) {
                if label.is_empty() {
                    continue;
                }
                if !defs.contains(&label) {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!("Reference label `{label}` is not defined"),
                    });
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M052ReferenceLinksImages;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_when_label_defined() {
        assert!(M052ReferenceLinksImages
            .lint(&f("See [home][hp].\n\n[hp]: https://x\n"))
            .is_empty());
    }
    #[test]
    fn flags_undefined_reference_label() {
        let v = M052ReferenceLinksImages.lint(&f("See [home][missing].\n"));
        assert_eq!(v.len(), 1);
    }
    #[test]
    fn ignores_inline_links() {
        assert!(M052ReferenceLinksImages
            .lint(&f("See [home](https://x).\n"))
            .is_empty());
    }
}
