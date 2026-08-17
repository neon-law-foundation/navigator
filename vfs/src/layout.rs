//! Where a document lands inside a mounted matter folder.
//!
//! A matter folder has exactly two halves, and the split **is** the
//! authorization boundary rather than an organizing convention:
//!
//! ```text
//! ~/Projects/<code>/
//!   client/      exactly the assets with visibility = 'client'
//!   internal/    exactly the assets with visibility = 'internal'
//! ```
//!
//! `assets.visibility` decides the half and nothing else does. The column
//! is a closed vocabulary enforced by `assets_visibility_check`, so this
//! module reads it directly instead of inferring a second rule from
//! `kind`, `source`, or a filename.
//!
//! Organizing by document kind happens *within* a half, never across one:
//! [`subfolder`] maps `assets.kind` to a directory, and an unmapped kind
//! lands at the half's root rather than being dropped. `kind` is
//! unconstrained free-form text in the database, so that mapping must be
//! total — there is no vocabulary to exhaust.
//!
//! # Failing closed
//!
//! [`Half::from_visibility`] returns `None` for a value it does not
//! recognize, and [`matter`] collects those rows into
//! [`Matter::unplaceable`] instead of guessing. Guessing has an asymmetric
//! cost here: a row misfiled into `internal/` is invisible, while a row
//! misfiled into `client/` is a document published to a client by
//! accident. Neither half is a safe default for a value that should not
//! exist, so an unrecognized row is reported and not rendered.

use uuid::Uuid;

use crate::tree::{self, Document, Entry};

/// The authorization half a document renders under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Half {
    /// `assets.visibility = 'client'` — the deliverable set.
    Client,
    /// `assets.visibility = 'internal'` — never leaves the firm.
    Internal,
}

impl Half {
    /// Read a half from an `assets.visibility` value.
    ///
    /// Returns `None` for anything outside the closed vocabulary; see the
    /// module docs for why that is not defaulted.
    #[must_use]
    pub fn from_visibility(visibility: &str) -> Option<Self> {
        match visibility {
            "client" => Some(Self::Client),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }

    /// The directory component this half renders as.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Internal => "internal",
        }
    }
}

/// The subfolder an `assets.kind` maps to within its half, or `None` to
/// place the document at the half's root.
///
/// The table is deliberately minimal. Most kinds the workspace writes
/// today (`retainer`, `will`, `trust`, `directive_health`, …) describe an
/// instrument whose correct directory depends on whether it has been
/// *executed*, not on the kind alone — an unexecuted retainer under
/// `signed/` would be a substantive misfiling, not a cosmetic one. Those
/// entries need the classification decision recorded on the epic before
/// they land here; until then they fall to the half's root, which is
/// visible and harmless.
#[must_use]
pub fn subfolder(kind: Option<&str>) -> Option<&'static str> {
    match kind? {
        // Correspondence regardless of execution state: an inbound message
        // and an outbound mailroom send are both letters.
        "message" | "mailroom_send" => Some("correspondence"),
        _ => None,
    }
}

/// The relative directory a document renders into, as
/// `client`, `internal/correspondence`, and so on.
///
/// Returns `None` when the visibility is unrecognized.
#[must_use]
pub fn place(visibility: &str, kind: Option<&str>) -> Option<String> {
    let half = Half::from_visibility(visibility)?;
    Some(match subfolder(kind) {
        Some(sub) => format!("{}/{sub}", half.as_str()),
        None => half.as_str().to_string(),
    })
}

/// One rendered directory within a matter folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    /// Path relative to the matter root — `client/correspondence`.
    pub path: String,
    /// The half this directory sits under, so a caller materializing for
    /// a restricted device can filter without re-parsing `path`.
    pub half: Half,
    /// Entries, named and collision-resolved within this directory.
    pub entries: Vec<Entry>,
}

/// One matter folder's full layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matter {
    /// Directories, sorted by path. Empty directories are not rendered —
    /// the tree describes rows that exist, never a fixed skeleton.
    pub directories: Vec<Directory>,
    /// Ids of rows whose `visibility` was not recognized. These are
    /// reported rather than rendered; see the module docs.
    pub unplaceable: Vec<Uuid>,
}

/// Render `documents` as one matter folder.
///
/// Collision resolution runs **per directory**, because that is the scope
/// in which two names actually collide: a `scan.pdf` under
/// `client/correspondence` and another under `internal` are not a
/// conflict and must not be suffixed as though they were.
#[must_use]
pub fn matter(documents: &[Document]) -> Matter {
    let mut buckets: Vec<(String, Half, Vec<Document>)> = Vec::new();
    let mut unplaceable = Vec::new();

    for document in documents {
        let Some(half) = Half::from_visibility(&document.visibility) else {
            unplaceable.push(document.id);
            continue;
        };
        let path = match subfolder(document.kind.as_deref()) {
            Some(sub) => format!("{}/{sub}", half.as_str()),
            None => half.as_str().to_string(),
        };

        match buckets.iter_mut().find(|(existing, ..)| *existing == path) {
            Some((_, _, group)) => group.push(document.clone()),
            None => buckets.push((path, half, vec![document.clone()])),
        }
    }

    buckets.sort_by(|(left, ..), (right, ..)| left.cmp(right));
    unplaceable.sort_unstable();

    Matter {
        directories: buckets
            .into_iter()
            .map(|(path, half, group)| Directory {
                path,
                half,
                entries: tree::directory(&group),
            })
            .collect(),
        unplaceable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: u128, filename: &str, visibility: &str, kind: Option<&str>) -> Document {
        Document {
            id: Uuid::from_u128(id),
            filename: Some(filename.to_string()),
            sha256_hex: format!("{id:064x}"),
            byte_size: 1,
            content_type: "application/pdf".to_string(),
            visibility: visibility.to_string(),
            kind: kind.map(str::to_string),
        }
    }

    #[test]
    fn visibility_decides_the_half() {
        assert_eq!(Half::from_visibility("client"), Some(Half::Client));
        assert_eq!(Half::from_visibility("internal"), Some(Half::Internal));
    }

    #[test]
    fn an_unrecognized_visibility_has_no_half() {
        // Fails closed rather than defaulting: see the module docs.
        assert_eq!(Half::from_visibility("public"), None);
        assert_eq!(Half::from_visibility(""), None);
        assert_eq!(Half::from_visibility("Client"), None);
    }

    #[test]
    fn an_unmapped_kind_lands_at_the_half_root() {
        assert_eq!(
            place("internal", Some("unclassified")).as_deref(),
            Some("internal")
        );
        assert_eq!(place("client", Some("retainer")).as_deref(), Some("client"));
        assert_eq!(place("client", None).as_deref(), Some("client"));
    }

    #[test]
    fn a_mapped_kind_picks_the_subfolder() {
        assert_eq!(
            place("internal", Some("message")).as_deref(),
            Some("internal/correspondence")
        );
        assert_eq!(
            place("client", Some("mailroom_send")).as_deref(),
            Some("client/correspondence")
        );
    }

    #[test]
    fn the_same_kind_maps_within_either_half() {
        // `kind` never crosses the boundary; it only organizes inside it.
        assert_eq!(subfolder(Some("message")), Some("correspondence"));
        assert_eq!(
            place("client", Some("message")).as_deref(),
            Some("client/correspondence")
        );
        assert_eq!(
            place("internal", Some("message")).as_deref(),
            Some("internal/correspondence")
        );
    }

    #[test]
    fn place_rejects_an_unrecognized_visibility() {
        assert_eq!(place("public", Some("message")), None);
    }

    #[test]
    fn a_matter_splits_into_halves() {
        let matter = matter(&[
            document(1, "brief.pdf", "internal", None),
            document(2, "receipt.pdf", "client", None),
        ]);

        let paths: Vec<&str> = matter.directories.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, vec!["client", "internal"]);
        assert_eq!(matter.directories[0].half, Half::Client);
        assert_eq!(matter.directories[1].half, Half::Internal);
        assert!(matter.unplaceable.is_empty());
    }

    #[test]
    fn empty_directories_are_not_rendered() {
        // The tree describes rows that exist; it is not a fixed skeleton.
        let matter = matter(&[document(1, "brief.pdf", "internal", None)]);
        assert_eq!(matter.directories.len(), 1);
        assert_eq!(matter.directories[0].path, "internal");
    }

    #[test]
    fn collisions_resolve_per_directory_not_per_matter() {
        // The same filename in two different directories is not a
        // collision and must not be suffixed as though it were.
        let matter = matter(&[
            document(1, "scan.pdf", "client", None),
            document(2, "scan.pdf", "internal", None),
        ]);

        assert_eq!(matter.directories[0].entries[0].name, "scan.pdf");
        assert_eq!(matter.directories[1].entries[0].name, "scan.pdf");
    }

    #[test]
    fn collisions_still_resolve_within_one_directory() {
        let matter = matter(&[
            document(1, "scan.pdf", "client", None),
            document(2, "scan.pdf", "client", None),
        ]);

        let names: Vec<&str> = matter.directories[0]
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, vec!["scan.pdf", "scan (2).pdf"]);
    }

    #[test]
    fn an_unplaceable_row_is_reported_not_rendered() {
        let matter = matter(&[
            document(1, "brief.pdf", "internal", None),
            document(2, "leak.pdf", "public", None),
        ]);

        assert_eq!(matter.directories.len(), 1);
        assert_eq!(matter.directories[0].path, "internal");
        assert_eq!(matter.unplaceable, vec![Uuid::from_u128(2)]);
    }

    #[test]
    fn an_unplaceable_row_never_falls_into_client() {
        let matter = matter(&[document(1, "leak.pdf", "", None)]);
        assert!(matter.directories.is_empty());
        assert_eq!(matter.unplaceable, vec![Uuid::from_u128(1)]);
    }

    #[test]
    fn the_layout_is_independent_of_row_order() {
        let forward = matter(&[
            document(1, "a.pdf", "client", Some("message")),
            document(2, "b.pdf", "internal", None),
            document(3, "c.pdf", "client", None),
        ]);
        let reversed = matter(&[
            document(3, "c.pdf", "client", None),
            document(2, "b.pdf", "internal", None),
            document(1, "a.pdf", "client", Some("message")),
        ]);

        assert_eq!(forward, reversed);
    }
}
