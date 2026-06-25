```mermaid
erDiagram
    addresses {
        UUID id PK
        UUID person_id FK
        UUID entity_id FK
        CHARACTER VARYING line1
        CHARACTER VARYING line2
        CHARACTER VARYING city
        CHARACTER VARYING region
        CHARACTER VARYING postal_code
        CHARACTER VARYING country
        TEXT inserted_at
        TEXT updated_at
    }
    answers {
        UUID id PK
        UUID question_id FK
        UUID person_id FK
        TEXT value
        TEXT inserted_at
        TEXT updated_at
        CHARACTER VARYING source
        UUID authored_by_person_id FK
    }
    attestations {
        UUID id PK
        UUID notation_id FK
        CHARACTER VARYING chain
        CHARACTER VARYING sha256
        CHARACTER VARYING status
        CHARACTER VARYING pda
        CHARACTER VARYING tx_signature
        CHARACTER VARYING firm_wallet
        CHARACTER VARYING client_wallet
        CHARACTER VARYING recorded_at
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    blobs {
        UUID id PK
        CHARACTER VARYING storage_key
        CHARACTER VARYING content_type
        BIGINT byte_size
        CHARACTER VARYING sha256_hex
        TEXT inserted_at
        TEXT updated_at
    }
    communications {
        UUID id PK
        UUID project_id FK
        CHARACTER VARYING channel
        CHARACTER VARYING direction
        UUID author_person_id FK
        CHARACTER VARYING counterparty
        CHARACTER VARYING subject
        TEXT body
        CHARACTER VARYING source_ref
        UUID blob_id FK
        CHARACTER VARYING occurred_at
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    contract_reviews {
        UUID id PK
        UUID notation_id FK
        UUID playbook_id FK
        UUID document_id FK
        CHARACTER VARYING status
        TEXT risk_summary
        JSONB findings
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    coupons {
        UUID id PK
        CHARACTER VARYING code
        INTEGER discount_percent
        BIGINT discount_amount_cents
        CHARACTER VARYING product_code
        CHARACTER VARYING expires_at
        INTEGER max_redemptions
        INTEGER redeemed_count
        BOOLEAN active
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    credentials {
        UUID id PK
        UUID person_id FK
        UUID jurisdiction_id FK
        CHARACTER VARYING license_number
        TEXT inserted_at
        TEXT updated_at
    }
    disclosures {
        UUID id PK
        UUID entity_id FK
        UUID project_id FK
        CHARACTER VARYING kind
        TEXT summary
        TEXT inserted_at
        TEXT updated_at
    }
    document_comments {
        UUID id PK
        UUID review_document_id FK
        UUID person_id FK
        INTEGER anchor_start
        INTEGER anchor_end
        TEXT quoted_text
        TEXT body
        BOOLEAN resolved
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
        UUID communication_id FK
    }
    documents {
        UUID id PK
        UUID project_id FK
        UUID blob_id FK
        CHARACTER VARYING filename
        CHARACTER VARYING kind
        TEXT inserted_at
        TEXT updated_at
        TEXT source
        TEXT source_revision_id
        TEXT received_at
        TEXT description
        CHARACTER VARYING git_commit_oid
    }
    email_conversation_messages {
        UUID id PK
        UUID conversation_id FK
        CHARACTER VARYING direction
        CHARACTER VARYING from_addr
        CHARACTER VARYING to_addr
        CHARACTER VARYING subject
        TEXT body_text
        CHARACTER VARYING raw_storage_key
        CHARACTER VARYING provider_message_id
        CHARACTER VARYING in_reply_to
        TEXT command_payload
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    email_conversations {
        UUID id PK
        CHARACTER VARYING token
        CHARACTER VARYING external_email
        CHARACTER VARYING external_name
        UUID person_id FK
        CHARACTER VARYING subject
        CHARACTER VARYING status
        UUID notation_id FK
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    email_tokens {
        UUID id PK
        UUID person_id FK
        CHARACTER VARYING email
        CHARACTER VARYING purpose
        CHARACTER VARYING token_hash
        CHARACTER VARYING expires_at
        CHARACTER VARYING used_at
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    entities {
        UUID id PK
        CHARACTER VARYING name
        UUID entity_type_id FK
        UUID jurisdiction_id FK
        TEXT inserted_at
        TEXT updated_at
        CHARACTER VARYING phone
        CHARACTER VARYING url
    }
    entity_billing_profiles {
        UUID id PK
        UUID entity_id FK
        CHARACTER VARYING billing_email
        UUID billing_address_id FK
        TEXT inserted_at
        TEXT updated_at
    }
    entity_types {
        UUID id PK
        CHARACTER VARYING name
        TEXT inserted_at
        TEXT updated_at
    }
    expunge_records {
        UUID id PK
        UUID project_id FK
        CHARACTER VARYING path
        CHARACTER VARYING category
        UUID authorized_by_person_id FK
        CHARACTER VARYING head_before
        CHARACTER VARYING head_after
        TEXT note
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    expunge_requests {
        UUID id PK
        UUID project_id FK
        UUID document_id FK
        UUID requested_by_person_id FK
        CHARACTER VARYING status
        TEXT note
        UUID resolved_by_person_id FK
        UUID expunge_record_id FK
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    filings {
        UUID id PK
        UUID notation_id FK
        CHARACTER VARYING kind
        CHARACTER VARYING office
        CHARACTER VARYING reference
        TEXT summary
        CHARACTER VARYING submitted_at
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    git_access_tokens {
        UUID id PK
        UUID person_id FK
        UUID project_id FK
        CHARACTER VARYING token_hash
        CHARACTER VARYING scope
        CHARACTER VARYING expires_at
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    git_repositories {
        UUID id PK
        CHARACTER VARYING remote_hash
        CHARACTER VARYING last_commit_sha
        TEXT inserted_at
        TEXT updated_at
    }
    invoice_line_items {
        UUID id PK
        UUID invoice_id FK
        CHARACTER VARYING description
        INTEGER quantity
        BIGINT unit_price_cents
        TEXT inserted_at
        TEXT updated_at
    }
    invoices {
        UUID id PK
        UUID entity_billing_profile_id FK
        CHARACTER VARYING number
        CHARACTER VARYING status
        BIGINT total_cents
        CHARACTER VARYING currency
        TEXT inserted_at
        TEXT updated_at
    }
    jurisdictions {
        UUID id PK
        CHARACTER VARYING name
        CHARACTER VARYING code
        TEXT inserted_at
        TEXT updated_at
        CHARACTER VARYING jurisdiction_type
    }
    letters {
        UUID id PK
        UUID mailroom_id FK
        CHARACTER VARYING direction
        CHARACTER VARYING sender
        CHARACTER VARYING recipient
        TEXT summary
        TEXT inserted_at
        TEXT updated_at
    }
    mailrooms {
        UUID id PK
        CHARACTER VARYING name
        UUID address_id FK
        TEXT inserted_at
        TEXT updated_at
    }
    notation_clauses {
        UUID id PK
        UUID notation_id FK
        INTEGER position
        TEXT body_markdown
        UUID authored_by_person_id FK
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    notation_events {
        UUID id PK
        UUID notation_id FK
        CHARACTER VARYING machine_kind
        CHARACTER VARYING from_state
        CHARACTER VARYING to_state
        CHARACTER VARYING condition
        TEXT payload
        CHARACTER VARYING recorded_at
        TEXT inserted_at
        TEXT updated_at
    }
    notations {
        UUID id PK
        UUID template_id FK
        UUID person_id FK
        UUID entity_id FK
        CHARACTER VARYING state
        UUID project_id FK
        TEXT inserted_at
        TEXT updated_at
        CHARACTER VARYING signature_request_id
        CHARACTER VARYING delivery
        INTEGER discount_pct
        BIGINT discount_amount_cents
        CHARACTER VARYING discount_reason
        CHARACTER VARYING discount_approved_by
        CHARACTER VARYING discount_approved_at
    }
    person_entity_roles {
        UUID id PK
        UUID person_id FK
        UUID entity_id FK
        CHARACTER VARYING role
        TEXT inserted_at
        TEXT updated_at
    }
    person_project_roles {
        UUID id PK
        UUID person_id FK
        UUID project_id FK
        CHARACTER VARYING participation
        TEXT inserted_at
        TEXT updated_at
    }
    persons {
        UUID id PK
        CHARACTER VARYING name
        CHARACTER VARYING email
        CHARACTER VARYING oidc_subject
        TEXT inserted_at
        TEXT updated_at
        TEXT role
        CHARACTER VARYING preferred_language
        CHARACTER VARYING title
        CHARACTER VARYING phone
        CHARACTER VARYING xero_contact_id
        CHARACTER VARYING profile_image_url
    }
    playbooks {
        UUID id PK
        UUID entity_id FK
        CHARACTER VARYING name
        JSONB positions
        BOOLEAN active
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    products {
        UUID id PK
        CHARACTER VARYING code
        CHARACTER VARYING display_name
        BIGINT list_price_cents
        CHARACTER VARYING currency
        CHARACTER VARYING cadence
        CHARACTER VARYING billing_kind
        BOOLEAN active
        CHARACTER VARYING xero_item_code
        CHARACTER VARYING matter_close_template_code
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
        CHARACTER VARYING account_code
        CHARACTER VARYING retainer_template_code
    }
    projects {
        UUID id PK
        CHARACTER VARYING name
        CHARACTER VARYING status
        UUID entity_id FK
        TEXT inserted_at
        TEXT updated_at
        CHARACTER VARYING git_initialized_at
        CHARACTER VARYING closed_at
        TEXT description
        UUID staff_dri_person_id FK
        UUID client_dri_person_id FK
    }
    question_translations {
        UUID id PK
        UUID question_id FK
        CHARACTER VARYING locale
        TEXT prompt
        TEXT help_text
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    questions {
        UUID id PK
        CHARACTER VARYING code
        TEXT prompt
        CHARACTER VARYING answer_type
        TEXT inserted_at
        TEXT updated_at
        CHARACTER VARYING audience
    }
    relationship_edges {
        UUID id PK
        CHARACTER VARYING from_type
        UUID from_id
        CHARACTER VARYING to_type
        UUID to_id
        CHARACTER VARYING kind
        INTEGER confidence_pct
        CHARACTER VARYING source_kind
        UUID source_id
        TEXT detail
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    relationship_logs {
        UUID id PK
        UUID actor_person_id FK
        CHARACTER VARYING subject_type
        UUID subject_id
        CHARACTER VARYING action
        TEXT detail
        TEXT inserted_at
        TEXT updated_at
    }
    review_documents {
        UUID id PK
        UUID notation_id FK
        CHARACTER VARYING kind
        CHARACTER VARYING title
        TEXT body_html
        CHARACTER VARYING status
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    sent_emails {
        UUID id PK
        CHARACTER VARYING recipient
        CHARACTER VARYING subject
        CHARACTER VARYING sender
        CHARACTER VARYING template_slug
        TEXT body
        CHARACTER VARYING outcome
        CHARACTER VARYING sent_at
        TEXT inserted_at
        TEXT updated_at
        CHARACTER VARYING sg_message_id
    }
    share_issuances {
        UUID id PK
        UUID entity_id FK
        CHARACTER VARYING holder_name
        CHARACTER VARYING share_class
        BIGINT shares
        CHARACTER VARYING issued_at
        TEXT inserted_at
        TEXT updated_at
    }
    statute_revisions {
        UUID id PK
        UUID statute_id FK
        TEXT body
        CHARACTER VARYING body_sha256
        CHARACTER VARYING section_title
        CHARACTER VARYING history_note
        CHARACTER VARYING observed_at
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    statutes {
        UUID id PK
        CHARACTER VARYING jurisdiction
        CHARACTER VARYING code
        CHARACTER VARYING chapter
        CHARACTER VARYING chapter_title
        CHARACTER VARYING section
        CHARACTER VARYING source_url
        CHARACTER VARYING status
        CHARACTER VARYING first_seen_at
        CHARACTER VARYING last_checked_at
        CHARACTER VARYING last_changed_at
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    subscriptions {
        UUID id PK
        UUID person_id
        UUID entity_id
        UUID project_id
        CHARACTER VARYING product_code
        CHARACTER VARYING contact_name
        CHARACTER VARYING contact_email
        CHARACTER VARYING status
        CHARACTER VARYING started_at
        CHARACTER VARYING last_invoiced_period
        INTEGER discount_percent
        BIGINT discount_amount_cents
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    templates {
        UUID id PK
        CHARACTER VARYING code
        CHARACTER VARYING title
        CHARACTER VARYING respondent_type
        TEXT inserted_at
        TEXT updated_at
        UUID project_id FK
        UUID blob_id FK
        CHARACTER VARYING form_code
    }
    testimonials {
        UUID id PK
        UUID project_id FK
        UUID person_id FK
        CHARACTER VARYING product_code FK
        TEXT quote
        CHARACTER VARYING attribution_label
        CHARACTER VARYING consented_at
        CHARACTER VARYING published_at
        INTEGER display_order
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    xero_invoices {
        UUID id PK
        UUID project_id FK
        CHARACTER VARYING xero_invoice_id
        CHARACTER VARYING reference
        CHARACTER VARYING status
        BIGINT amount_cents
        BIGINT amount_paid_cents
        CHARACTER VARYING currency
        CHARACTER VARYING inserted_at
        CHARACTER VARYING updated_at
    }
    persons ||--o{ addresses : "person_id"
    entities ||--o{ addresses : "entity_id"
    questions ||--o{ answers : "question_id"
    persons ||--o{ answers : "person_id"
    persons ||--o{ answers : "authored_by_person_id"
    notations ||--o{ attestations : "notation_id"
    projects ||--o{ communications : "project_id"
    persons ||--o{ communications : "author_person_id"
    blobs ||--o{ communications : "blob_id"
    notations ||--o{ contract_reviews : "notation_id"
    playbooks ||--o{ contract_reviews : "playbook_id"
    documents ||--o{ contract_reviews : "document_id"
    persons ||--o{ credentials : "person_id"
    jurisdictions ||--o{ credentials : "jurisdiction_id"
    entities ||--o{ disclosures : "entity_id"
    projects ||--o{ disclosures : "project_id"
    review_documents ||--o{ document_comments : "review_document_id"
    persons ||--o{ document_comments : "person_id"
    communications ||--o{ document_comments : "communication_id"
    projects ||--o{ documents : "project_id"
    blobs ||--o{ documents : "blob_id"
    email_conversations ||--o{ email_conversation_messages : "conversation_id"
    persons ||--o{ email_conversations : "person_id"
    notations ||--o{ email_conversations : "notation_id"
    persons ||--o{ email_tokens : "person_id"
    entity_types ||--o{ entities : "entity_type_id"
    jurisdictions ||--o{ entities : "jurisdiction_id"
    entities ||--o{ entity_billing_profiles : "entity_id"
    addresses ||--o{ entity_billing_profiles : "billing_address_id"
    projects ||--o{ expunge_records : "project_id"
    persons ||--o{ expunge_records : "authorized_by_person_id"
    projects ||--o{ expunge_requests : "project_id"
    documents ||--o{ expunge_requests : "document_id"
    persons ||--o{ expunge_requests : "requested_by_person_id"
    persons ||--o{ expunge_requests : "resolved_by_person_id"
    expunge_records ||--o{ expunge_requests : "expunge_record_id"
    notations ||--o{ filings : "notation_id"
    persons ||--o{ git_access_tokens : "person_id"
    projects ||--o{ git_access_tokens : "project_id"
    invoices ||--o{ invoice_line_items : "invoice_id"
    entity_billing_profiles ||--o{ invoices : "entity_billing_profile_id"
    mailrooms ||--o{ letters : "mailroom_id"
    addresses ||--o{ mailrooms : "address_id"
    notations ||--o{ notation_clauses : "notation_id"
    persons ||--o{ notation_clauses : "authored_by_person_id"
    notations ||--o{ notation_events : "notation_id"
    templates ||--o{ notations : "template_id"
    persons ||--o{ notations : "person_id"
    entities ||--o{ notations : "entity_id"
    projects ||--o{ notations : "project_id"
    persons ||--o{ person_entity_roles : "person_id"
    entities ||--o{ person_entity_roles : "entity_id"
    persons ||--o{ person_project_roles : "person_id"
    projects ||--o{ person_project_roles : "project_id"
    entities ||--o{ playbooks : "entity_id"
    entities ||--o{ projects : "entity_id"
    persons ||--o{ projects : "staff_dri_person_id"
    persons ||--o{ projects : "client_dri_person_id"
    questions ||--o{ question_translations : "question_id"
    persons ||--o{ relationship_logs : "actor_person_id"
    notations ||--o{ review_documents : "notation_id"
    entities ||--o{ share_issuances : "entity_id"
    statutes ||--o{ statute_revisions : "statute_id"
    projects ||--o{ templates : "project_id"
    blobs ||--o{ templates : "blob_id"
    projects ||--o{ testimonials : "project_id"
    persons ||--o{ testimonials : "person_id"
    products ||--o{ testimonials : "product_code"
    projects ||--o{ xero_invoices : "project_id"

```
