---
title: Simple Last Will and Testament
respondent_type: person
code: will__simple
jurisdiction: NV
confidential: true
prompts:
  testator_name: What is your full legal name?
  executor_name: Who is the executor of your will?
  residuary_beneficiary: Who receives the remainder of your estate?
questionnaire:
  BEGIN:
    _: custom_text__testator_name
  custom_text__testator_name:
    _: custom_text__executor_name
  custom_text__executor_name:
    _: custom_text__residuary_beneficiary
  custom_text__residuary_beneficiary:
    _: END
  END: {}
workflow:
  BEGIN:
    _: testator_signature
  testator_signature:
    _: witnesses
  witnesses:
    _: staff_review
  staff_review:
    _: notarization
  notarization:
    _: END
  END: {}
---

I, `{{custom_text__testator_name}}`, declare this to be my Last Will and Testament. I name
`{{custom_text__executor_name}}` as the executor of my estate and direct that the residue pass to
`{{custom_text__residuary_beneficiary}}`.
