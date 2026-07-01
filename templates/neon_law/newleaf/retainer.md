---
title: Newleaf Engagement Agreement
respondent_type: person
code: onboarding__retainer_newleaf
jurisdiction: NV
confidential: true
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
prompts:
  client_name: What is the client's full legal name?
  client_email: What is the client's email address?
  project_name: What is the project name for this engagement?
  product_description: Describe the services this retainer covers.
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
`{{person__client.name}}` (the "Client"), reachable at `{{person__client.email}}`, for **Neon Law Newleaf** —
an uncontested divorce — on the matter referred to as `{{project__engagement.name}}`.

**The work and the fee.** The Firm will prepare and file the documents to dissolve a marriage where both spouses have
already agreed on every term — the division of property and debts, any support, and any arrangements for children:
`{{custom_text__product_description}}`. This is a flat fee for the uncontested dissolution, billed once when the matter
closes; the court's filing fees and any service-of-process costs are passed through at cost on top of the flat fee.

**Scope of the engagement.** The Firm's representation is limited to preparing and filing the uncontested dissolution
documents described above and in the clauses of this Agreement. If the divorce becomes contested — any genuine
disagreement about property, debts, support, or children — the flat-fee engagement ends, and continued work requires a
separate written engagement or a written amendment to this one signed by the Client and the Firm.

Either party may terminate this Agreement upon written notice. The Client remains responsible for fees and expenses
incurred prior to termination.

{{custom_clauses}}

**We represent both of you — your conflict waiver.** Even an amicable divorce is legally adverse, so representing both
spouses is a conflict the law allows only with your informed, written consent (RPC 1.7). By signing, each of you agrees
that the Firm represents both of you jointly; that we prepare and file only the dissolution you have already agreed on;
that we cannot advise or advocate for either of you against the other; and that there is no confidentiality between you
— anything one spouse tells the Firm about this matter may be shared with the other (RPC 1.6). This engagement is
limited to your uncontested dissolution (RPC 1.2(c)). "Uncontested" means you agree on every term: property, debts,
support, and any children. If either of you disagrees on any of it, the matter is no longer uncontested, the Firm
withdraws from representing both of you, and each of you should retain independent counsel. You are each encouraged to
consult independent counsel before signing this waiver.

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

By initialing here, the Client acknowledges that the Firm represents both spouses jointly in this uncontested
dissolution, that there is no confidentiality between the spouses and the Firm cannot take either side against the
other, and that the Firm withdraws from both if the divorce becomes contested; any contested matter requires separate
counsel: {{client.initials}}

{{firm.signature}}

{{firm.date}}
