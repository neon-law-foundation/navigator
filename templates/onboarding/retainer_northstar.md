---
title: Northstar Engagement Agreement
respondent_type: person
code: onboarding__retainer_northstar
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
"Client"), reachable at `{{client_email}}`, for **Neon Law Northstar** — estate planning — on the matter referred to as
`{{project_name}}`.

**The work and the fee.** The Firm prepares your estate plan — a will, a revocable living trust, and the health-care and
financial directives that go with it — from one recorded sitting: `{{product_description}}`. This is one flat fee for
the plan, billed once when the matter closes; any recording, notarization, or filing costs are passed through at cost.

**Scope of the engagement.** The Firm's representation is limited to preparing the estate-planning instruments and the
document work described above and in the clauses of this Agreement. Funding the trust beyond the instruments we prepare,
a probate proceeding, a tax controversy, or any later amendment requires a separate written engagement or a written
amendment to this one signed by both the Client and the Firm.

Either party may terminate this Agreement upon written notice. The Client remains responsible for fees and expenses
incurred prior to termination.

{{custom_clauses}}

**Representing you, and — if you ask us to — your spouse or partner together.** Many couples want one firm to prepare
their plans together. We can do that, but you should understand what it means before you agree. When the Firm represents
two people jointly on their estate plans, **there are no secrets between you as to this matter**: what one of you tells
the Firm about the plan we may share with the other, because under RPC 1.6 and RPC 1.4 we cannot keep information from a
client we represent. We represent your shared interest in the plan and, under RPC 1.7, **cannot take one of you against
the other**; if a real conflict opens up between you, the joint representation ends and each of you may retain
independent counsel. If a person we represent comes to have difficulty making or communicating decisions about the plan,
the Firm will, so far as reasonably possible, maintain a normal client relationship and act to protect that person's
interests, as RPC 1.14 directs. Finally, under RPC 1.8(c) the Firm will **not** prepare an instrument that leaves a
substantial gift to the Firm or any of its lawyers, and will not name itself or its lawyers to a paid fiduciary role
under an instrument it drafts — your executor, trustee, and agents are people you choose.

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

By initialing here, the Client acknowledges that this engagement covers the flat-fee preparation of the estate-planning
instruments described above, and does not include trust funding beyond those instruments, probate, or a tax controversy;
any such matter requires a separate written engagement with the Firm: {{client.initials}}

{{firm.signature}}

{{firm.date}}
