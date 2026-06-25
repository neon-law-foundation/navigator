//! Blank government forms — the logged-in `/portal/forms` index.
//!
//! Lists every vendored form from the `forms` registry: the exact
//! canonical bytes the workflows fill, downloadable as blanks. The
//! handler shapes registry entries into [`FormRow`]s so the view does
//! not depend on the `forms` crate and stays trivial to test.

use maud::{html, Markup};

use crate::PageLayout;

/// One vendored form as the index renders it.
pub struct FormRow {
    pub form_code: String,
    pub name: String,
    pub authority: String,
    pub revision: String,
    pub retrieved: String,
    pub source_url: String,
}

/// Render the `/portal/forms` index.
#[must_use]
pub fn index(rows: &[FormRow]) -> Markup {
    let body = html! {
        section."portal" {
            div.container {
                h1."h3"."mb-2" { "Blank government forms" }
                p."text-body-secondary"."mb-4" {
                    "The official forms Neon Law Navigator fills — vendored from each authority's "
                    "own site and pinned by revision. Download a blank to read what a "
                    "filing asks before you answer the questionnaire; your matter's "
                    "filled copy always goes through attorney review."
                }
                div."table-responsive" {
                    table."table"."align-middle" {
                        thead {
                            tr {
                                th { "Form" }
                                th { "Authority" }
                                th { "Revision" }
                                th { "Vendored" }
                                th { "" }
                            }
                        }
                        tbody {
                            @for row in rows {
                                tr {
                                    td {
                                        (row.name)
                                        br;
                                        code."small" { (row.form_code) }
                                    }
                                    td { (row.authority) }
                                    td { (row.revision) }
                                    td {
                                        (row.retrieved)
                                        " · "
                                        a href=(row.source_url) rel="noopener noreferrer" { "source" }
                                    }
                                    td {
                                        a."btn"."btn-sm"."btn-outline-primary"
                                            href=(format!("/portal/forms/{}.pdf", row.form_code)) {
                                            "Download blank"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new("Blank government forms — Neon Law Navigator")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{index, FormRow};

    fn row() -> FormRow {
        FormRow {
            form_code: "nv_sos__llc_formation".into(),
            name: "Limited-Liability Company Formation Packet (NRS 86)".into(),
            authority: "Nevada Secretary of State".into(),
            revision: "2023-08".into(),
            retrieved: "2026-06-12".into(),
            source_url: "https://www.nvsos.gov/businesses".into(),
        }
    }

    #[test]
    fn lists_the_form_with_a_download_link() {
        let html = index(&[row()]).into_string();
        assert!(html.contains("Limited-Liability Company Formation Packet"));
        assert!(html.contains("/portal/forms/nv_sos__llc_formation.pdf"));
        assert!(html.contains("Nevada Secretary of State"));
        assert!(html.contains("2023-08"));
    }
}
