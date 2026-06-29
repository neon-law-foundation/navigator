//! `navigator docs ...` — command-line access to published workspace docs.
//!
//! The glossary is parsed from `docs/glossary.md` at compile time so the CLI
//! cannot drift from the website's published vocabulary.

use std::process::ExitCode;

use crate::palette;

const GLOSSARY_MD: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/glossary.md"));

#[derive(Debug, Clone, PartialEq, Eq)]
struct GlossaryEntry {
    title: String,
    slug: String,
    body: String,
}

#[must_use]
pub fn list() -> ExitCode {
    let docs = web::docs::loader::bundled();
    for doc in docs.docs() {
        println!("/docs/{slug}\t{title}", slug = doc.slug, title = doc.title);
    }
    for entry in glossary_entries() {
        println!(
            "/docs/glossary#{slug}\tGlossary: {title}",
            slug = entry.slug,
            title = entry.title,
        );
    }
    ExitCode::SUCCESS
}

#[must_use]
pub fn glossary(term: Option<&str>) -> ExitCode {
    let entries = glossary_entries();
    let Some(needle) = term else {
        for entry in &entries {
            print_entry(entry);
        }
        return ExitCode::SUCCESS;
    };
    if let Some(entry) = entries.iter().find(|entry| matches_entry(entry, needle)) {
        print_entry(entry);
        ExitCode::SUCCESS
    } else {
        eprintln!("navigator: docs glossary: unknown term `{needle}`");
        eprintln!("Run `navigator docs list` to list every published docs page.");
        ExitCode::from(1)
    }
}

fn print_entry(entry: &GlossaryEntry) {
    println!("## {}", palette::header(&entry.title));
    println!();
    println!("{}", entry.body.trim());
    println!();
}

fn matches_entry(entry: &GlossaryEntry, needle: &str) -> bool {
    entry.title.eq_ignore_ascii_case(needle) || entry.slug == slugify_heading(needle)
}

fn glossary_entries() -> Vec<GlossaryEntry> {
    let mut entries = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();
    for line in GLOSSARY_MD.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if let Some(title) = current_title.replace(title.trim().to_string()) {
                entries.push(entry(title, &current_body));
                current_body.clear();
            }
        } else if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(title) = current_title {
        entries.push(entry(title, &current_body));
    }
    entries
}

fn entry(title: String, body: &str) -> GlossaryEntry {
    GlossaryEntry {
        slug: slugify_heading(&title),
        title,
        body: body.trim().to_string(),
    }
}

fn slugify_heading(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == ' ' {
            out.push('-');
        } else if c == '-' || c == '_' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{glossary_entries, slugify_heading};

    #[test]
    fn parses_glossary_headings_as_entries() {
        let entries = glossary_entries();
        assert!(entries.iter().any(|entry| entry.title == "Project"));
        assert!(entries.iter().any(|entry| entry.title == "Staff Review"));
    }

    #[test]
    fn heading_slug_matches_published_docs_anchor_shape() {
        assert_eq!(slugify_heading("Staff Review"), "staff-review");
        assert_eq!(
            slugify_heading("Engagement / Retainer"),
            "engagement--retainer"
        );
    }
}
