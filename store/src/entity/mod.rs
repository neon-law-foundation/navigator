//! SeaORM entities for the web crate's persistent model.
//!
//! Each submodule defines one table — a `Model` row type, the
//! generated `Entity`, an `ActiveModel` for inserts/updates, and any
//! relations.

pub mod address;
pub mod answer;
pub mod attestation;
pub mod blob;
pub mod communication;
pub mod contract_review;
pub mod coupon;
pub mod credential;
pub mod disclosure;
pub mod document;
pub mod document_comment;
pub mod email_conversation;
pub mod email_conversation_message;
pub mod email_token;
pub mod entity;
pub mod entity_billing_profile;
pub mod entity_type;
pub mod event;
pub mod expunge_record;
pub mod expunge_request;
pub mod filing;
pub mod git_access_token;
pub mod git_repository;
pub mod invoice;
pub mod invoice_line_item;
pub mod jurisdiction;
pub mod letter;
pub mod mailroom;
pub mod notarization;
pub mod notation;
pub mod notation_clause;
pub mod notation_event;
pub mod person;
pub mod person_entity_role;
pub mod person_project_role;
pub mod playbook;
pub mod product;
pub mod project;
pub mod question;
pub mod question_translation;
pub mod relationship_edge;
pub mod relationship_log;
pub mod review_document;
pub mod sent_email;
pub mod share_issuance;
pub mod signature;
pub mod statute;
pub mod statute_revision;
pub mod subscription;
pub mod template;
pub mod testimonial;
pub mod xero_invoice;
