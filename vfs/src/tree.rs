//! Rendering a set of asset rows as one directory.
//!
//! `(project_id, slug)` is deliberately non-unique in the `assets` table,
//! and `filename` is not constrained at all, so the rows destined for one
//! directory routinely want the same name: two uploads called `scan.pdf`,
//! or a `Retainer.pdf` alongside a `retainer.pdf`. A directory cannot hold
//! both, and the case-insensitive filesystems the firm actually runs on —
//! APFS and NTFS — cannot even hold the second pair.
//!
//! [`directory`] resolves that the way Finder and Explorer do, by suffixing
//! ` (2)`, ` (3)`, and so on. The assignment is a pure function of the row
//! set, so a given matter renders identically on every device and across
//! remounts; nothing depends on the order the database returned rows in.

use uuid::Uuid;

use crate::name;

/// One authorization-filtered `assets` row, reduced to what naming needs.
/// The caller has already decided this row is visible to this person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub id: Uuid,
    /// `assets.filename`; `None` for a bare content asset.
    pub filename: Option<String>,
    /// `assets.sha256_hex` — the content-addressed cache key. Blobs are
    /// immutable, so a cached file under this key never goes stale.
    pub sha256_hex: String,
    pub byte_size: u64,
    pub content_type: String,
    /// `assets.visibility` — `internal` or `client`. Decides which half of
    /// the matter folder this row renders under; see [`crate::layout`].
    pub visibility: String,
    /// `assets.kind` (`retainer`, `message`, …); `None` for a bare content
    /// asset. Picks the subfolder *within* a half, never the half itself.
    pub kind: Option<String>,
}

/// One rendered directory entry: a [`Document`] under the unique, legal
/// name this directory will show it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub id: Uuid,
    pub sha256_hex: String,
    pub byte_size: u64,
    pub content_type: String,
}

/// Render `documents` as one directory, assigning every row a name that is
/// legal on macOS and Windows and unique within the directory even under
/// case-insensitive comparison.
///
/// Entries come back sorted by name, which is both a stable order for the
/// mount to serve and what makes suffix assignment independent of the
/// caller's query order.
pub fn directory(documents: &[Document]) -> Vec<Entry> {
    let mut candidates: Vec<(String, &Document)> = documents
        .iter()
        .map(|document| {
            let raw = document.filename.as_deref().unwrap_or(name::FALLBACK);
            (name::sanitize(raw), document)
        })
        .collect();

    // The id breaks ties so two rows with identical filenames still get a
    // deterministic suffix each, rather than one that flips between reads.
    candidates.sort_by(|(left, left_doc), (right, right_doc)| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
            .then_with(|| left_doc.id.cmp(&right_doc.id))
    });

    // Collision detection folds case because APFS and NTFS do. It does not
    // yet fold Unicode normalization, so an NFC and NFD spelling of one
    // name still collide on macOS; no observed row pair differs that way.
    let mut taken: Vec<String> = Vec::with_capacity(candidates.len());
    let mut entries = Vec::with_capacity(candidates.len());

    for (preferred, document) in candidates {
        let mut chosen = preferred.clone();
        let mut index = 2;

        while taken.contains(&chosen.to_lowercase()) {
            chosen = with_index(&preferred, index);
            index += 1;
        }

        taken.push(chosen.to_lowercase());
        entries.push(Entry {
            name: chosen,
            id: document.id,
            sha256_hex: document.sha256_hex.clone(),
            byte_size: document.byte_size,
            content_type: document.content_type.clone(),
        });
    }

    entries
}

/// Insert ` (index)` before the extension, the way both desktop file
/// managers disambiguate, trimming the stem first so the result still fits
/// in a path component.
fn with_index(base: &str, index: usize) -> String {
    let suffix = format!(" ({index})");

    let (stem, extension) = match base.rfind('.').filter(|dot| *dot > 0) {
        Some(dot) => (&base[..dot], &base[dot..]),
        None => (base, ""),
    };

    let budget = name::MAX_NAME_BYTES.saturating_sub(suffix.len() + extension.len());
    let stem = name::truncate_on_boundary(stem, budget)
        .trim_end_matches(['.', ' '])
        .trim_end();

    if stem.is_empty() {
        return format!("{}{suffix}{extension}", name::FALLBACK);
    }

    format!("{stem}{suffix}{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: u128, filename: Option<&str>) -> Document {
        Document {
            id: Uuid::from_u128(id),
            filename: filename.map(String::from),
            sha256_hex: format!("{id:064x}"),
            byte_size: 1_024,
            content_type: "application/pdf".to_string(),
            // `directory` renders one already-chosen directory, so neither
            // column participates here; the split lives in `layout`.
            visibility: "internal".to_string(),
            kind: None,
        }
    }

    fn names(documents: &[Document]) -> Vec<String> {
        directory(documents)
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    #[test]
    fn distinct_names_are_left_alone() {
        let rendered = names(&[
            document(1, Some("Retainer.pdf")),
            document(2, Some("Complaint.pdf")),
        ]);

        assert_eq!(rendered, vec!["Complaint.pdf", "Retainer.pdf"]);
    }

    #[test]
    fn duplicates_are_suffixed_before_the_extension() {
        let rendered = names(&[
            document(1, Some("scan.pdf")),
            document(2, Some("scan.pdf")),
            document(3, Some("scan.pdf")),
        ]);

        assert_eq!(rendered, vec!["scan.pdf", "scan (2).pdf", "scan (3).pdf"]);
    }

    #[test]
    fn collisions_fold_case_like_apfs_and_ntfs() {
        // Both would be a single file on the filesystems the firm runs on.
        let rendered = names(&[
            document(1, Some("Retainer.pdf")),
            document(2, Some("retainer.pdf")),
        ]);

        assert_eq!(rendered, vec!["Retainer.pdf", "retainer (2).pdf"]);
    }

    #[test]
    fn an_existing_suffixed_name_is_not_overwritten() {
        // Somebody really did upload a file called `scan (2).pdf`.
        let rendered = names(&[
            document(1, Some("scan.pdf")),
            document(2, Some("scan (2).pdf")),
            document(3, Some("scan.pdf")),
        ]);

        assert_eq!(rendered, vec!["scan (2).pdf", "scan.pdf", "scan (3).pdf"]);
        assert_eq!(
            rendered.len(),
            rendered
                .iter()
                .map(|name| name.to_lowercase())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "every rendered name is unique"
        );
    }

    #[test]
    fn extensionless_names_take_a_trailing_suffix() {
        let rendered = names(&[document(1, Some("Notes")), document(2, Some("Notes"))]);

        assert_eq!(rendered, vec!["Notes", "Notes (2)"]);
    }

    #[test]
    fn bare_content_assets_fall_back_and_still_disambiguate() {
        let rendered = names(&[document(1, None), document(2, None)]);

        assert_eq!(rendered, vec![name::FALLBACK, "unnamed (2)"]);
    }

    #[test]
    fn illegal_names_are_sanitized_then_disambiguated() {
        // Both sanitize to the same legal name, so one must yield.
        let rendered = names(&[
            document(1, Some("Smith: notes.pdf")),
            document(2, Some("Smith* notes.pdf")),
        ]);

        assert_eq!(rendered, vec!["Smith_ notes.pdf", "Smith_ notes (2).pdf"]);
    }

    #[test]
    fn assignment_is_independent_of_input_order() {
        let forward = [
            document(3, Some("scan.pdf")),
            document(1, Some("scan.pdf")),
            document(2, Some("scan.pdf")),
        ];
        let mut reversed = forward.clone();
        reversed.reverse();

        // Same row set, different query order, same folder on disk.
        assert_eq!(directory(&forward), directory(&reversed));
    }

    #[test]
    fn suffixing_keeps_the_name_within_the_component_limit() {
        let long = format!("{}.pdf", "a".repeat(300));
        let rendered = names(&[
            document(1, Some(&long)),
            document(2, Some(&long)),
            document(3, Some(&long)),
        ]);

        for name in &rendered {
            assert!(name.len() <= name::MAX_NAME_BYTES, "{name} is too long");
            assert!(name.ends_with(".pdf"));
        }
        assert_eq!(
            rendered
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3,
            "truncation did not collapse the three names into one"
        );
    }

    #[test]
    fn an_empty_directory_renders_empty() {
        assert!(directory(&[]).is_empty());
    }

    #[test]
    fn entries_carry_the_cache_key_and_metadata_through() {
        let entries = directory(&[document(7, Some("Order.pdf"))]);

        assert_eq!(entries[0].id, Uuid::from_u128(7));
        assert_eq!(entries[0].sha256_hex, format!("{:064x}", 7));
        assert_eq!(entries[0].byte_size, 1_024);
        assert_eq!(entries[0].content_type, "application/pdf");
    }
}
