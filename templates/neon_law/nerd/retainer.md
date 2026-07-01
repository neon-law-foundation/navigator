---
title: Expert-Witness Engagement Agreement
respondent_type: person_and_entity
code: onboarding__retainer_nerd
jurisdiction: US
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
This Engagement Agreement (the "Agreement") is entered into between Neon Law's expert-witness practice, **Neon Law
Nerd** (the "Firm"), and `{{person__client.name}}` (the "Client"), reachable at
`{{person__client.email}}`, for the expert-witness and litigation-consulting matter referred to as
`{{project__engagement.name}}`.

**The work and the fee.** The Firm provides expert analysis, a written report, and — where the engagement calls for it —
testimony by deposition or at trial, on the software and data-access matter described here:
`{{custom_text__product_description}}`.
This is an evaluation undertaken for use by you and, where you designate, the tribunal and other parties (RPC 2.3). The
Firm's work is billed by the hour at `$1,337` per hour against the engagement's rate sheet, with costs and expenses
passed through at cost. **The fee is earned for the time and analysis and is never contingent on the conclusions the
Firm reaches, on the substance of any opinion or testimony, or on who prevails in the matter** — a contingent expert fee
is prohibited, and the Firm will not accept one (RPC 3.4). The Firm gives its **independent** professional opinion based
on the materials it reviews; it does not guarantee any particular opinion, finding, or result.

**Candor and independence.** An expert offered by one side still owes the tribunal candor — the Firm will state what the
evidence and its analysis actually show, will not present testimony it knows to be false, and will tell you candidly
when the facts do not support the position you hoped for (RPC 3.3 and RPC 3.4). You are paying for a credible,
independent analysis; that credibility is the value of the engagement, and the Firm will not trade it away.

**Scope of the engagement.** The Firm's representation is limited to the expert-witness and consulting work described
above and in the clauses of this Agreement (RPC 1.2(c)). A separate matter, an appeal, a new proceeding, or testimony in
a forum not named here requires a separate written engagement or a written amendment to this one signed by both the
Client and the Firm.

Either party may terminate this Agreement upon written notice, subject to the rules governing a lawyer's withdrawal and
an expert's obligations in a pending matter. The Client remains responsible for fees and expenses incurred prior to
termination.

{{custom_clauses}}

**Confidentiality and the materials you share.** The Firm holds the information and materials you share in confidence
(RPC 1.6) and uses them only for this engagement, subject to the disclosure obligations that attach once an expert is
designated to testify — a testifying expert's report, the materials relied on, and the bases for the opinions are
generally discoverable, and the Firm cannot promise confidentiality that the rules of the forum do not allow. Tell us
before you share anything you intend to keep privileged so we can handle it correctly.

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
matter flow through it. The Firm sends invoices and case correspondence to you at `{{person__client.email}}`;
you reach the Firm at support@neonlaw.com.

**Firm-wide conflicts.** Neon Law is a small firm, and we treat a conflict for any one of our attorneys as a conflict
for the entire firm. Before we take on a new matter, we check it against all of our current and former matters across
every attorney here. If that check turns up a conflict we cannot properly take on, we will tell you promptly, decline
the matter rather than wall it off internally, refer you to outside counsel, and return any materials you shared with
us. The Firm neither pays nor accepts a referral fee on any matter it refers out. By engaging us, you acknowledge that
our attorneys share matter information among themselves for this purpose.

**Your file, kept for ten years.** The Firm keeps your complete matter file — every document, the materials you shared,
the report and exhibits we prepared, and the privileged correspondence we exchange with you — for ten years after your
matter closes. You may request a copy of your file at any point during that period. After ten years, the Firm securely
destroys the file and its contents.

The Client acknowledges receipt of the Firm's privacy notice and agrees to electronic delivery of invoices and case
correspondence at `{{person__client.email}}`.

The Client and the Firm execute this Agreement electronically as of the dates signed below.

{{client.signature}}

{{client.date}}

By initialing here, the Client acknowledges that this engagement is expert-witness and consulting work billed by the
hour, that the fee is never contingent on the opinion reached or the outcome of the matter, and that a separate
proceeding or new testimony requires a separate written engagement with the Firm: {{client.initials}}

{{firm.signature}}

{{firm.date}}
