---
title: Nevada Trust
respondent_type: entity
code: trusts__nevada
jurisdiction: NV
confidential: false
questionnaire:
  BEGIN:
    _: trustee_name
  trustee_name:
    _: trust_property
  trust_property:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__trustee
  intake_persisted__trustee:
    trust_rendered: staff_review
  staff_review:
    approved: document_open__trust_pdf
    rejected: END
  document_open__trust_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
    signature_declined: END
  END: {}
---

This Revocable Living Trust Agreement (the "Trust") is established under the laws of the State of Nevada by the settlor,
who also serves as trustee, `{{trustee_name}}` (the "Trustee"). The Trustee holds the following property as the corpus
of the Trust: `{{trust_property}}`.

The Trust is revocable: the settlor may amend or revoke it in whole or in part at any time during the settlor's lifetime
by a signed writing delivered to the Trustee. The Trustee holds and administers the trust property for the benefit of
the beneficiaries the settlor names, and distributes it according to the settlor's instructions.

**How real property is funded into this Trust.** This Trust is valid and takes effect on the settlor's signature alone —
Nevada does not require witnesses or a notary for the trust instrument itself. Funding **real property** into the Trust
is different: a deed transferring real estate into the Trust must be signed before a notary and recorded with the county
recorder to be effective. Neon Law prepares and records that deed as a separate, notarized step; it is **not** part of
this electronic signing. Signing this Trust does not by itself move any real property into it.

The settlor establishes this Trust and the firm acknowledges its preparation as of the dates signed below.

{{client.signature}}

{{client.date}}

{{firm.signature}}

{{firm.date}}
