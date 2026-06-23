//! Canonical URL slug convention — kebab-case everywhere.
//!
//! Borrowing the JSON:API member-name convention, every public URL slug
//! for a file-backed asset (a blog post, a notation template, a
//! workspace doc) uses hyphens, never underscores. Files on disk keep
//! their underscore names — they are referenced by `store::seed`, `cli
//! validate`, and the notation `code` field — so the `_`→`-` mapping
//! happens only at the URL boundary, in exactly two moves:
//!
//! 1. **Link generation** runs every file-derived path segment through
//!    [`to_url`], so a rendered `href` is always kebab-case.
//! 2. **Routing** permanently redirects a legacy underscore request to
//!    its kebab form ([`needs_redirect`] + [`to_url`]), then matches the
//!    incoming segment against the on-disk name by comparing the kebab
//!    form of *both* sides.
//!
//! The mapping is deliberately one-way at lookup time: `_`→`-` is not
//! invertible (a real filename may itself contain a hyphen), so a caller
//! resolving a kebab slug back to a file must compare normalized forms
//! rather than try to reverse the slug.

/// The canonical URL form of a file-derived identifier: underscores
/// become hyphens. Idempotent — an already-kebab slug is unchanged, so
/// it is safe to apply at both link-generation and lookup time.
#[must_use]
pub fn to_url(name: &str) -> String {
    name.replace('_', "-")
}

/// Whether `slug` is the legacy underscore form and a request for it
/// should be permanently redirected to [`to_url`]`(slug)`. False for an
/// already-kebab slug, so the redirect fires at most once.
#[must_use]
pub fn needs_redirect(slug: &str) -> bool {
    slug.contains('_')
}

#[cfg(test)]
mod tests {
    use super::{needs_redirect, to_url};

    #[test]
    fn to_url_replaces_underscores_with_hyphens() {
        assert_eq!(to_url("thanks_apple"), "thanks-apple");
        assert_eq!(to_url("form990_annual_report"), "form990-annual-report");
        assert_eq!(to_url("nv_state_tax_filing"), "nv-state-tax-filing");
    }

    #[test]
    fn to_url_is_idempotent_for_kebab_and_plain_slugs() {
        assert_eq!(to_url("thanks-apple"), "thanks-apple");
        assert_eq!(to_url("glossary"), "glossary");
        // A filename that legitimately contains a hyphen is left intact —
        // the mapping never round-trips a hyphen back to an underscore.
        assert_eq!(to_url("rust-in-peace"), "rust-in-peace");
    }

    #[test]
    fn needs_redirect_only_for_underscore_slugs() {
        assert!(needs_redirect("thanks_apple"));
        assert!(needs_redirect("nv_state_tax_filing"));
        assert!(!needs_redirect("thanks-apple"));
        assert!(!needs_redirect("glossary"));
    }
}
