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
    _: person__testator
  person__testator:
    _: person__executor
  person__executor:
    _: person__residuary_beneficiary
  person__residuary_beneficiary:
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

I, `{{person__testator.name}}`, declare this to be my Last Will and Testament. I name
`{{person__executor.name}}` as the executor of my estate and direct that the residue pass to
`{{person__residuary_beneficiary.name}}`.
