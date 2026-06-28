---
title: Nautilus Engagement Agreement
respondent_type: person
code: onboarding__retainer_nautilus
jurisdiction: US
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: client_email
  client_email:
    _: project_name
  project_name:
    _: product_description
  product_description:
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
This Engagement Agreement (the "Agreement") is entered into between Neon Law (the "Firm") and `{{client_name}}` (the
"Client"), reachable at `{{client_email}}`, for **Neon Law Nautilus** — debt-collection correspondence and consumer
rights work — on the matter referred to as `{{project_name}}`.

**The work and the fee.** The Firm handles correspondence with debt collectors and credit bureaus on your behalf and
asserts your rights under the federal Fair Debt Collection Practices Act (FDCPA) and Fair Credit Reporting Act (FCRA):
`{{product_description}}`. This is a flat monthly fee. The Firm does not take any percentage of any amount a debt is
reduced or settled, and it charges no separate fee for a settlement.

**Scope of the engagement — limited on purpose.** This is a **limited-scope** engagement (RPC 1.2(c)). It covers written
correspondence and the assertion of your consumer rights — sending a notice of representation, a debt-validation or
cease-communication demand, and FCRA disputes — **and it does not include litigation, court appearances, bankruptcy, or
representation in any lawsuit**, whether one is filed by you or against you. If a collector sues you or litigation
becomes necessary or advisable, the Firm will tell you, and that work requires a separate written engagement or a
referral to litigation counsel.

Either party may terminate this Agreement upon written notice. The Client remains responsible for fees and expenses
incurred prior to termination.

{{custom_clauses}}

**What we can and cannot promise.** The flat monthly fee is the Firm's fee for the limited-scope correspondence and
rights work described above, and we believe it is reasonable for that work (RPC 1.5). The Firm **does not guarantee any
particular result** — that a debt will be reduced, removed, settled, or deleted from your credit report, or that a
collector will stop contacting you (RPC 7.1). What the Firm does guarantee is that a licensed attorney handles your
correspondence and asserts the rights the law gives you. Once the Firm notifies a collector that it represents you, that
collector must, under the FDCPA, direct its communications to the Firm rather than to you.

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
matter flow through it. The Firm sends invoices and case correspondence to you at `{{client_email}}`; you reach the Firm
at support@neonlaw.com.

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
correspondence at `{{client_email}}`.

The Client and the Firm execute this Agreement electronically as of the dates signed below.

{{client.signature}}

{{client.date}}

By initialing here, the Client acknowledges that this is a limited-scope engagement covering debt-collection
correspondence and consumer-rights work for a flat monthly fee, that it does not include litigation, court appearances,
or bankruptcy, and that the Firm guarantees no particular result; any litigation requires a separate written engagement
or a referral: {{client.initials}}

{{firm.signature}}

{{firm.date}}
