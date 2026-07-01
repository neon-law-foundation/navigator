---
title: Namesake Engagement Agreement
respondent_type: person_and_entity
code: onboarding__retainer_namesake
jurisdiction: US
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
`{{person__client.name}}` (the "Client"), reachable at `{{person__client.email}}`, for
**Neon Law Namesake** — a United States trademark application — on the matter referred to as
`{{project__engagement.name}}`.

**The work and the fee.** The Firm will prepare and file a trademark application with the United States Patent and
Trademark Office (USPTO) for one class of goods or services, and handle the routine correspondence to register it:
`{{custom_text__product_description}}`. This is a flat fee for one class, billed once when the matter closes; the USPTO
filing fee for the class is passed through at cost, and each additional class is a separate flat fee.

**Scope of the engagement.** The Firm's representation is limited to preparing and filing the application in one class
and the routine prosecution to registration described above and in the clauses of this Agreement. A substantive refusal,
a third-party opposition, and the later maintenance or renewal filings are each outside this engagement and require a
separate written engagement or a written amendment to this one signed by both the Client and the Firm.

Either party may terminate this Agreement upon written notice. The Client remains responsible for fees and expenses
incurred prior to termination.

{{custom_clauses}}

**What a flat-fee filing covers.** The Firm files your application competently and in the class you choose (RPC 1.1),
but the engagement is limited to that filing and the routine prosecution to registration (RPC 1.2(c)) — it does not
include a substantive refusal, a third-party opposition, or the later maintenance filings, each of which needs a
separate engagement. A registration is not guaranteed; the USPTO decides whether the mark registers. We will keep you
informed of the office actions and deadlines on your application and tell you promptly what each one requires of you
(RPC 1.4).

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

By initialing here, the Client acknowledges that this engagement covers the flat-fee preparation and filing of a
trademark application in one class and the routine prosecution to registration, that registration is not guaranteed, and
that it does not include a refusal, an opposition, additional classes, or maintenance filings; any such matter requires
a separate written engagement with the Firm: {{client.initials}}

{{firm.signature}}

{{firm.date}}
