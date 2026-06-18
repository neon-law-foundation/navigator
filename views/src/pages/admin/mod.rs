//! Admin section views.

pub mod archives;
pub mod cap_table;
pub mod clauses;
pub mod contract_reviews;
pub mod coupons;
pub mod dashboard;
pub mod email_log;
pub mod entities;
pub mod entity_types;
pub mod expunge;
pub mod expunge_requests;
pub mod generic_list;
pub mod letters;
pub mod people;
pub mod playbooks;
pub mod projects;
pub mod questions;
pub mod retainers;
pub mod schedules;
pub mod subscriptions;
pub mod templates;

pub use dashboard::{dashboard, DashboardCounts};
pub use generic_list::{render as render_list, render_load_error, ListPage};
