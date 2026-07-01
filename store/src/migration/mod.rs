//! Schema migrations, run at boot.
//!
//! `Migrator::up(db, None)` brings the database forward to the
//! latest schema. Each migration lives in its own file and the
//! `migrations()` list is the canonical ordered history.

mod m20260519_create_persons;
mod m20260520_create_entities;
mod m20260520_create_entity_types;
mod m20260520_create_jurisdictions;
mod m20260521_create_workflow_tables;
mod m20260522_create_mail_tables;
mod m20260523_create_project_tables;
mod m20260524_create_document_tables;
mod m20260525_create_billing_tables;
mod m20260526_create_provenance_tables;
mod m20260527_add_oidc_subject_to_persons;
mod m20260528_add_roles_to_persons;
mod m20260529_create_credentials;
mod m20260530_create_share_issuances;
mod m20260601_create_notation_events;
mod m20260602_create_sent_emails;
mod m20260610_add_project_id_to_notations;
mod m20260611_create_project_ingestions;
mod m20260612_add_timestamps_to_all_tables;
mod m20260613_add_drive_folder_id_to_projects;
mod m20260616_create_drive_syncs;
mod m20260617_rename_project_ingestion_commit_sha;
mod m20260618_collapse_project_ingestions_into_documents;
mod m20260619_collapse_persons_roles_to_role;
mod m20260620_add_sg_message_id_to_sent_emails;
mod m20260621_add_signature_request_id_to_notations;
mod m20260622_create_filings;
mod m20260623_add_intake_language;
mod m20260624_template_storage_and_scoping;
mod m20260625_create_review_tables;
mod m20260626_create_email_conversation_tables;
mod m20260627_add_git_repo_to_projects;
mod m20260628_add_git_commit_oid_to_documents;
mod m20260629_create_expunge_records;
mod m20260630_create_expunge_requests;
mod m20260701_add_contact_fields;
mod m20260702_create_statutes;
mod m20260703_add_answer_authorship_and_question_audience;
mod m20260704_create_notation_clauses;
mod m20260705_create_communications;
mod m20260706_add_closed_at_to_projects;
mod m20260707_create_xero_invoices;
mod m20260708_add_delivery_to_notations;
mod m20260709_create_products;
mod m20260710_add_discount_to_notations;
mod m20260711_add_description_to_projects;
mod m20260712_projects_entity_id_not_null;
mod m20260713_drop_drive_folder_id_from_projects;
mod m20260714_add_account_code_to_products;
mod m20260715_create_subscriptions;
mod m20260716_add_form_code_to_templates;
mod m20260717_add_retainer_template_code_to_products;
mod m20260718_drop_drive_syncs;
mod m20260719_drop_git_default_branch_from_projects;
mod m20260720_create_coupons;
mod m20260721_create_contract_review_tables;
mod m20260722_create_attestations;
mod m20260723_create_email_tokens;
mod m20260724_add_jurisdiction_type_to_jurisdictions;
mod m20260725_add_project_dri_columns;
mod m20260726_create_testimonials;
mod m20260727_create_relationship_edges;
mod m20260728_create_events;
mod m20260729_answers_notation_scoped_jsonb;
mod m20260730_template_versions;
mod m20260731_add_questionnaire_snapshot_to_notations;
mod m20260801_create_signatures_and_notarizations;

pub struct Migrator;

#[async_trait::async_trait]
impl sea_orm_migration::MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
        vec![
            Box::new(m20260519_create_persons::Migration),
            Box::new(m20260520_create_jurisdictions::Migration),
            Box::new(m20260520_create_entity_types::Migration),
            Box::new(m20260520_create_entities::Migration),
            Box::new(m20260521_create_workflow_tables::Migration),
            Box::new(m20260522_create_mail_tables::Migration),
            Box::new(m20260523_create_project_tables::Migration),
            Box::new(m20260524_create_document_tables::Migration),
            Box::new(m20260525_create_billing_tables::Migration),
            Box::new(m20260526_create_provenance_tables::Migration),
            Box::new(m20260527_add_oidc_subject_to_persons::Migration),
            Box::new(m20260528_add_roles_to_persons::Migration),
            Box::new(m20260529_create_credentials::Migration),
            Box::new(m20260530_create_share_issuances::Migration),
            Box::new(m20260601_create_notation_events::Migration),
            Box::new(m20260602_create_sent_emails::Migration),
            Box::new(m20260610_add_project_id_to_notations::Migration),
            Box::new(m20260611_create_project_ingestions::Migration),
            Box::new(m20260612_add_timestamps_to_all_tables::Migration),
            Box::new(m20260613_add_drive_folder_id_to_projects::Migration),
            Box::new(m20260616_create_drive_syncs::Migration),
            Box::new(m20260617_rename_project_ingestion_commit_sha::Migration),
            Box::new(m20260618_collapse_project_ingestions_into_documents::Migration),
            Box::new(m20260619_collapse_persons_roles_to_role::Migration),
            Box::new(m20260620_add_sg_message_id_to_sent_emails::Migration),
            Box::new(m20260621_add_signature_request_id_to_notations::Migration),
            Box::new(m20260622_create_filings::Migration),
            Box::new(m20260623_add_intake_language::Migration),
            Box::new(m20260624_template_storage_and_scoping::Migration),
            Box::new(m20260625_create_review_tables::Migration),
            Box::new(m20260626_create_email_conversation_tables::Migration),
            Box::new(m20260627_add_git_repo_to_projects::Migration),
            Box::new(m20260628_add_git_commit_oid_to_documents::Migration),
            Box::new(m20260629_create_expunge_records::Migration),
            Box::new(m20260630_create_expunge_requests::Migration),
            Box::new(m20260701_add_contact_fields::Migration),
            Box::new(m20260702_create_statutes::Migration),
            Box::new(m20260703_add_answer_authorship_and_question_audience::Migration),
            Box::new(m20260704_create_notation_clauses::Migration),
            Box::new(m20260705_create_communications::Migration),
            Box::new(m20260706_add_closed_at_to_projects::Migration),
            Box::new(m20260707_create_xero_invoices::Migration),
            Box::new(m20260708_add_delivery_to_notations::Migration),
            Box::new(m20260709_create_products::Migration),
            Box::new(m20260710_add_discount_to_notations::Migration),
            Box::new(m20260711_add_description_to_projects::Migration),
            Box::new(m20260712_projects_entity_id_not_null::Migration),
            Box::new(m20260713_drop_drive_folder_id_from_projects::Migration),
            Box::new(m20260714_add_account_code_to_products::Migration),
            Box::new(m20260715_create_subscriptions::Migration),
            Box::new(m20260716_add_form_code_to_templates::Migration),
            Box::new(m20260717_add_retainer_template_code_to_products::Migration),
            Box::new(m20260718_drop_drive_syncs::Migration),
            Box::new(m20260719_drop_git_default_branch_from_projects::Migration),
            Box::new(m20260720_create_coupons::Migration),
            Box::new(m20260721_create_contract_review_tables::Migration),
            Box::new(m20260722_create_attestations::Migration),
            Box::new(m20260723_create_email_tokens::Migration),
            Box::new(m20260724_add_jurisdiction_type_to_jurisdictions::Migration),
            Box::new(m20260725_add_project_dri_columns::Migration),
            Box::new(m20260726_create_testimonials::Migration),
            Box::new(m20260727_create_relationship_edges::Migration),
            Box::new(m20260728_create_events::Migration),
            Box::new(m20260729_answers_notation_scoped_jsonb::Migration),
            Box::new(m20260730_template_versions::Migration),
            Box::new(m20260731_add_questionnaire_snapshot_to_notations::Migration),
            Box::new(m20260801_create_signatures_and_notarizations::Migration),
        ]
    }
}
