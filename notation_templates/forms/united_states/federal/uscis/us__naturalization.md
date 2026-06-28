---
title: Application for Naturalization — Form N-400 Intake Summary
respondent_type: person
code: us__naturalization
jurisdiction: US
origin_url: https://www.uscis.gov/n-400
confidential: true
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: client_email
  client_email:
    _: date_of_birth
  date_of_birth:
    _: country_of_birth
  country_of_birth:
    _: country_of_citizenship
  country_of_citizenship:
    _: a_number
  a_number:
    _: lpr_since
  lpr_since:
    _: daytime_phone
  daytime_phone:
    _: eligibility_basis
  eligibility_basis:
    _: marital_status
  marital_status:
    _: time_outside_us
  time_outside_us:
    _: good_moral_character
  good_moral_character:
    _: END
  END: {}
workflow:
  BEGIN:
    intake_submitted: intake_persisted__applicant
  intake_persisted__applicant:
    application_rendered: staff_review
  staff_review:
    approved: document_open__n400_summary
    rejected: END
  document_open__n400_summary:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: e_filing__uscis
    signature_declined: END
  e_filing__uscis:
    filed: mailroom_receive__biometrics_notice
  mailroom_receive__biometrics_notice:
    received: mailroom_receive__interview_notice
  mailroom_receive__interview_notice:
    received: mailroom_receive__oath_notice
  mailroom_receive__oath_notice:
    certificate_received: document_intake__certificate_of_naturalization
  document_intake__certificate_of_naturalization:
    certificate_filed: END
  END: {}
---

This naturalization engagement (the "Engagement") prepares and files Form N-400, Application for Naturalization, with
U.S. Citizenship and Immigration Services ("USCIS") on behalf of `{{client_name}}` (the "Applicant").

The Applicant was born on `{{date_of_birth}}` in `{{country_of_birth}}`, is a citizen or national of
`{{country_of_citizenship}}`, and became a lawful permanent resident on `{{lpr_since}}`. The Applicant's Alien
Registration Number is `{{a_number}}`. The Applicant is `{{marital_status}}` and applies under the
`{{eligibility_basis}}` path to naturalization.

This summary records what the Applicant told the firm at intake so it can be reviewed before anything is filed. It is
not the application itself and is not legal advice. The firm prepares the full Form N-400 from these answers, and a
licensed Neon Law attorney reviews the completed application with the Applicant before it is signed. Nothing reaches
USCIS unreviewed, and the firm does not promise any particular outcome — USCIS alone decides the application.

After the Applicant signs, the firm files the Form N-400 with USCIS and stays with the Applicant through each step that
follows: the biometrics appointment, the interview and civics test, and the oath ceremony. The Engagement concludes when
USCIS issues the Applicant's Certificate of Naturalization (Form N-550) — the lifelong proof of U.S. citizenship.

Appointment notices and confirmations are sent to the Applicant at `{{client_email}}`, and the firm reaches the
Applicant by phone at `{{daytime_phone}}`. The Applicant reported roughly `{{time_outside_us}}` days outside the United
States in the last five years; the attorney reviews the exact travel dates against the continuous-residence requirement
before filing.

The Applicant signs below to confirm these intake answers are true and complete to the best of the Applicant's
knowledge, and the firm countersigns to open the matter.

{{client.signature}}

{{client.date}}

{{firm.signature}}

{{firm.date}}
