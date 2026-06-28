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
    pub code: String,
    pub title: String,
    pub jurisdiction: String,
    pub origin_url: String,
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
                    "own site and stored at the same path used in the public assets bucket. "
                    "Download a blank to read what a "
                    "filing asks before you answer the questionnaire; your matter's "
                    "filled copy always goes through attorney review."
                }
                div."table-responsive" {
                    table."table"."align-middle" {
                        thead {
                            tr {
                                th { "Form" }
                                th { "Jurisdiction" }
                                th { "Origin" }
                                th { "" }
                            }
                        }
                        tbody {
                            @for row in rows {
                                tr {
                                    td {
                                        (row.title)
                                        br;
                                        code."small" { (row.code) }
                                    }
                                    td {
                                        (row.jurisdiction)
                                    }
                                    td {
                                        a href=(row.origin_url) rel="noopener noreferrer" { "government website" }
                                    }
                                    td {
                                        a."btn"."btn-sm"."btn-outline-primary"
                                            href=(format!("/portal/forms/{}.pdf", row.code)) {
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
            code: "nv__llc_formation".into(),
            title: "Nevada LLC Formation".into(),
            jurisdiction: "NV".into(),
            origin_url: "https://www.nvsos.gov/businesses".into(),
        }
    }

    #[test]
    fn lists_the_form_with_a_download_link() {
        let html = index(&[row()]).into_string();
        assert!(html.contains("Nevada LLC Formation"));
        assert!(html.contains("/portal/forms/nv__llc_formation.pdf"));
        assert!(html.contains("NV"));
        assert!(html.contains("government website"));
    }
}
