---
title: Nook Engagement Agreement
respondent_type: person_and_entity
code: onboarding__retainer_nook
jurisdiction: NV
confidential: true
prompts:
  client_name: What is the client's full legal name?
  client_email: What is the client's email address?
  project_name: What is the project name for this engagement?
  product_description: Describe the services this retainer covers.
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: project__engagement
  project__engagement:
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
`{{person__client.name}}` (the "Client"), reachable at `{{person__client.email}}`, for **Neon Law Nook** — a
brokerless real-estate closing — on the matter referred to as `{{project__engagement.name}}`.

**The work and the fee.** For a sale the buyer and seller have already agreed on, with no broker on either side, the
Firm drafts the purchase agreement from the terms you have agreed on, prepares the deed and the closing documents,
coordinates the closing and the settlement of funds, and records the deed with the county:
`{{custom_text__product_description}}`.
This is one flat legal fee — `$9,999` — paid once when the matter closes, not a percentage of the sale price. County
recording fees and any transfer tax are billed at cost on top of the flat fee.

**Scope of the engagement.** The Firm's representation is limited to papering and closing the agreed sale described
above and in the clauses of this Agreement. Negotiating the deal, a title dispute, a financing matter, or any litigation
requires a separate written engagement or a written amendment to this one signed by both the Client and the Firm.

Either party may terminate this Agreement upon written notice. The Client remains responsible for fees and expenses
incurred prior to termination.

{{custom_clauses}}

**Who we represent — your choice, and the conflict rule behind it.** You choose. The Firm can represent you as the
**buyer**, represent you as the **seller**, or — when both of you want one firm to handle the whole closing — represent
the two of you **together**. When the Firm represents only one side, the other party **is not our client**; we tell them
so and suggest they consider their own counsel, and we give them no legal advice (RPC 4.3). When the Firm represents
**both** sides, we do so only with **each of you giving informed consent in writing**, because one firm holding both
sides of a deal means we **cannot advocate one of you against the other** (RPC 1.7) — our job is to close the sale you
both already agreed on, not to negotiate one of you up against the other. If a real dispute opens up between you over
the terms, the joint representation ends and each of you gets independent counsel — and we will say so plainly. This
reading matches what we publish about Nook on our website.

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
matter flow through it. The Firm sends invoices and case correspondence to you at `{{person__client.email}}`; you
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
correspondence at `{{person__client.email}}`.

The Client and the Firm execute this Agreement electronically as of the dates signed below.

{{client.signature}}

{{client.date}}

By initialing here, the Client acknowledges that this engagement covers the flat-fee drafting and closing of an
already-agreed sale, that where the Firm represents both buyer and seller it does so only with each party's informed
written consent and cannot advocate one against the other, and that it does not include negotiating the deal or any
litigation; any such matter requires a separate written engagement with the Firm: {{client.initials}}

{{firm.signature}}

{{firm.date}}
