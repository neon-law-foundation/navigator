---
# STUB BODY — the questionnaire + workflow below are the real, tested contract.
# This is the first review-IN matter: the client uploads a third-party contract, the
# `analysis__contract_deviations` step (web, Vertex Gemini) measures it against the client
# Entity's playbook, an attorney reviews every finding at `staff_review`, and the firm delivers
# a review memo (rendered web-side from the approved findings into `document_open__review_memo`).
# The prose body frames the engagement and carries the load-bearing disclaimers; the dynamic
# findings/risk-summary live in the web-assembled memo, NOT in this body (they are not
# questionnaire answers). The full engagement prose gets attorney review WITHOUT touching the flow.
title: Neon Law — Inbound Contract Review
respondent_type: person_and_entity
code: services__contract_review
jurisdiction: NV
confidential: true
prompts:
  client_name: What is the client's full legal name?
  entity_name: What is the legal name of your LLC?
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: entity__company
  entity__company:
    _: END
  END: {}
workflow:
  BEGIN:
    contract_uploaded: document_intake__inbound_contract
  document_intake__inbound_contract:
    intake_filed: analysis__contract_deviations
  analysis__contract_deviations:
    analysis_ready: staff_review
  staff_review:
    approved: document_open__review_memo
    rejected: END
  document_open__review_memo:
    memo_rendered: END
  END: {}
---

# Inbound contract review for {{entity__company.name}}

Neon Law reviews an inbound contract `{{entity__company.name}}` (the "Company") received from a third party, on
behalf of `{{person__client.name}}`. The Company uploads the contract; the firm measures it against the Company's
negotiation playbook, a licensed attorney reviews every point, and the firm delivers a written review memo to the
Company's Project repository.

## What this review covers

The memo measures the contract against the **specific playbook** the Company has on file — the positions the Company has
decided it wants on liability, term, renewal, governing law, and the other topics the playbook names. The memo flags
where the contract departs from those positions, ranks each departure, and suggests language to propose back.

## What this review does not do

This is a review against the Company's playbook, **not a full audit** of the contract. A clause the memo does not flag
is not thereby approved — silence means the clause was outside the playbook's scope, not that the firm blessed it. The
memo is the firm's advice on the points the playbook covers; the decision to sign remains the Company's.

A licensed Neon Law attorney reviews and signs off on every point in the memo before it reaches the Company. No finding
is delivered on the strength of an automated screen alone — the attorney is accountable for each one.

## How your contract is handled

To produce the review, the contract text is processed through the firm's contract-analysis tooling, which uses a
zero-retention AI service that is not trained on Company data. The contract and the memo are confidential, stored in the
Company's Project repository.
