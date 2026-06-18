#![allow(clippy::doc_markdown)]
//! Reusable view components.
//!
//! Each function returns a `Markup` snippet that the page-level
//! views compose into a body — the small atoms (FormField,
//! SelectField, SubmitButton, …) that hold the rest of the UI
//! together.

use maud::{html, Markup};

pub mod card;
pub mod code;
pub mod confirm_delete;
pub mod data_table;
pub mod disclaimer;
pub mod form;
pub mod freshness;
pub mod icon;
pub mod links;
pub mod pagination;
pub mod people_list;
pub mod pricing;
pub mod row_actions;
pub mod social;
pub mod sort_spec;
pub mod toast;

pub use card::Card;
pub use code::{code_block, syntax_highlight_assets};
pub use confirm_delete::ConfirmDeleteForm;
pub use data_table::{data_table, raw_cell, text_cell, Column};
pub use disclaimer::legal_blueprint_disclaimer;
pub use form::{Choice, Field, FieldKind, FormCard, Heading};
pub use icon::{product_icon, LIBRA_SCALES};
pub use links::{external_link, external_link_with_class, ExternalLink};
pub use pagination::pagination;
pub use people_list::people_list_inputs;
pub use pricing::{pricing_section, PricingCard};
pub use row_actions::RowActions;
pub use social::{social_meta, SocialMeta};
pub use sort_spec::{SortDirection, SortError, SortField, SortSpec};
pub use toast::{toast_overlay, Toast, ToastTone};

/// Render a small inline form-level error as a Bootstrap alert.
/// [`FormCard`] renders its own banner; this is the standalone atom
/// for the few places that show an error outside a `FormCard`.
#[must_use]
pub fn form_error(message: &str) -> Markup {
    html! { div."alert"."alert-danger" role="alert" { (message) } }
}

#[cfg(test)]
mod tests {
    use super::form_error;

    #[test]
    fn form_error_includes_role_and_text() {
        let html = form_error("Bad input").into_string();
        assert!(html.contains("role=\"alert\""));
        assert!(html.contains("Bad input"));
    }
}
