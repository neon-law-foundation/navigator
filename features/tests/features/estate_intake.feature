Feature: Northstar estate-plan workflow shape

  The estate plan is one notation, onboarding__estate, that carries one
  recorded sitting through transcript intake, structured-input extraction,
  attorney review, the client's comment-only approval, and signing. The
  sitting is transcribed offline by Ada on Google Gemini Enterprise and
  the transcript is uploaded through the reusable document-intake step —
  no live speech-to-text. The retainer is BDD-tested end-to-end through
  the walker; this branching, named-condition machine is pinned by its
  routes here, like the Nevada trust. The client_review state is the new
  reusable primitive — the demand-side mirror of staff_review: the client
  approves attorney-reviewed drafts before the plan goes to signature.

  Scenario: The estate workflow routes from uploaded transcript to signature
    Then the onboarding__estate workflow routes:
      | from                         | condition           | to                           |
      | BEGIN                        | transcript_uploaded | document_intake__transcript  |
      | document_intake__transcript  | transcript_ready    | extract__inputs              |
      | extract__inputs              | inputs_ready        | document_drafts__estate      |
      | document_drafts__estate      | drafts_persisted    | staff_review                 |
      | staff_review                 | approved            | client_review                |
      | client_review                | client_approved     | sent_for_signature__pending  |
      | sent_for_signature__pending  | signature_received  | END                          |

  Scenario: The attorney can reject before any client sees a draft
    Then the onboarding__estate workflow routes:
      | from         | condition | to  |
      | staff_review | rejected  | END |

  Scenario: A declined signature still ends the matter
    Then the onboarding__estate workflow routes:
      | from                        | condition          | to  |
      | sent_for_signature__pending | signature_declined | END |

  Scenario: Every estate workflow state resolves to a StepKind
    Then every onboarding__estate workflow state resolves to a StepKind

  Scenario: The estate questionnaire captures recording consent first
    Then the onboarding__estate questionnaire routes:
      | from              | condition | to                |
      | BEGIN             | _         | recording_consent |
      | recording_consent | _         | testator_name     |
