---
title: Node Engagement Agreement
respondent_type: person_and_entity
code: onboarding__retainer_node
jurisdiction: US
confidential: true
prompts:
  client_name: What is the client's full legal name?
  client_email: What is the client's email address?
  project_name: What is the project name for this engagement?
  product_description: Describe the services this retainer covers.
questionnaire:
  BEGIN:
    _: custom_text__client_name
  custom_text__client_name:
    _: custom_text__client_email
  custom_text__client_email:
    _: custom_text__project_name
  custom_text__project_name:
    _: custom_text__product_description
  custom_text__product_description:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__client
  intake_persisted__client:
    retainer_rendered: staff_review
  staff_review:
    approved: document_open__retainer_pdf
    rejected: END
  document_open__retainer_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
    signature_declined: END
  END: {}
---
This Engagement Agreement (the "Agreement") is entered into between Neon Law (the "Firm") and
`{{custom_text__client_name}}` (the "Client"), reachable at `{{custom_text__client_email}}`, for **Neon Law Node** — an
on-chain attorney attestation — on the matter referred to as `{{custom_text__project_name}}`.

**The work and the fee.** The Firm will confirm a legal fact about the Client — for example, the Client's status as an
accredited investor — **based solely on the information and documents the Client provides**, and record that attestation
on the Solana blockchain: a transaction binding the Firm's wallet, the Client's wallet, and a cryptographic hash of the
signed attestation document: `{{custom_text__product_description}}`. This is a flat fee per attestation, billed once
when the matter closes; any Solana network fees are passed through at cost on top of the flat fee.

**Scope of the engagement.** The Firm's representation is limited to confirming the stated fact and recording the single
on-chain attestation described above and in the clauses of this Agreement. The Firm relies **solely** on the information
and documents the Client provides and does **not** independently investigate, audit, or cross-reference them against any
outside source; the Client is responsible for their truth and completeness. Re-attestation, ongoing monitoring of the
attested fact, or a separate reliance opinion to another third party requires a separate written engagement or a written
amendment to this one signed by both the Client and the Firm.

Either party may terminate this Agreement upon written notice. The Client remains responsible for fees and expenses
incurred prior to termination.

{{custom_clauses}}

**An attestation others will rely on.** A Node attestation is an evaluation the Firm provides knowing a third party — an
issuer, a counterparty, or a smart contract — will rely on it (RPC 2.3). The evaluation rests solely on the information
the Client provides; the Firm does not independently investigate or cross-reference it, and the attestation says so on
its face. We make it only when doing so is compatible with our duties to you, and only after you give informed consent
to the specific facts we will publish, because the attestation and its on-chain hash necessarily disclose information
relating to the representation (RPC 1.6). Our engagement is limited to confirming the stated fact and recording the
single attestation described above (RPC 1.2(c)); we do not opine on anything beyond it, and the attestation speaks only
as of the date we record it.

**Resolving a dispute — binding arbitration.** If a dispute arises out of or relates to this engagement or this
Agreement, you and the Firm agree to resolve it by binding arbitration administered by **JAMS** under its Comprehensive
Arbitration Rules & Procedures — or, where the amount in controversy is small enough to qualify, its Streamlined Rules.
The arbitration is seated in **Reno, Nevada**, conducted confidentially, and decided under Nevada law; each party bears
its share of the JAMS fees as those rules provide. By agreeing to arbitration, you and the Firm give up the right to a
jury trial and to have the dispute decided in court — except as stated in the next paragraph. The arbitrator applies the
same law and may award the same remedies a court could; this clause selects the forum for a dispute and does **not**
limit, cap, or waive the Firm's responsibility for its own work. Because this is an agreement about how future disputes
are handled, you have the right to consult independent counsel of your own choosing before you agree to it.

**Your fee-arbitration rights are preserved.** Nothing in the arbitration clause waives or overrides any non-waivable
statutory right you have to arbitration of a fee dispute — including, in California, the Mandatory Fee Arbitration Act
(Bus. & Prof. Code § 6200 et seq.), and the corresponding fee-dispute programs of the State Bar of Nevada and the
Washington State Bar Association. You keep those rights in full.

**Reaching the Firm.** Email to **support@neonlaw.com** is the best and primary way to reach the Firm. You consent to
electronic communication at that address and understand that routine correspondence, documents, and questions about your
matter flow through it. The Firm sends invoices and case correspondence to you at `{{custom_text__client_email}}`; you
reach the Firm at support@neonlaw.com.

**Firm-wide conflicts.** Neon Law is a small firm, and we treat a conflict for any one of our attorneys as a conflict
for the entire firm. Before we take on a new matter, we check it against all of our current and former matters across
every attorney here. If that check turns up a conflict we cannot properly take on, we will tell you promptly, decline
the matter rather than wall it off internally, refer you to outside counsel, and return any materials you shared with
us. The Firm neither pays nor accepts a referral fee on any matter it refers out. By engaging us, you acknowledge that
our attorneys share matter information among themselves for this purpose.

**Your file, kept for ten years.** The Firm keeps your complete matter file — every document, signed agreement, and the
privileged correspondence we exchange with you — for ten years after your matter closes. You may request a copy of your
file at any point during that period. After ten years, the Firm securely destroys the file and its contents.

The Client acknowledges receipt of the Firm's privacy notice and agrees to electronic delivery of invoices and case
correspondence at `{{custom_text__client_email}}`.

The Client and the Firm execute this Agreement electronically as of the dates signed below.

{{client.signature}}

{{client.date}}

By initialing here, the Client acknowledges that this engagement covers the flat-fee verification and the single
on-chain attestation described above, that the attestation speaks only as of the date recorded, and that it does not
include re-attestation, monitoring, or any other legal work; any such matter requires a separate written engagement with
the Firm: {{client.initials}}

{{firm.signature}}

{{firm.date}}
