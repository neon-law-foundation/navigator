Feature: Nevada trust rides the generalized e-signature send path

  The retainer was the first template wired for e-signature. The Nevada
  revocable trust is the second — and the first to prove the send path
  is no longer retainer-specific: the same walker + post-questionnaire
  drive resolve the workflow spec, storage keys, and captive-signer
  identity from the notation's template, not a hardcoded retainer.

  A walked trust renders the trust instrument with anchored settlor +
  attorney signature blocks and parks at sent_for_signature__pending
  with a provider envelope id — exactly the retainer's shape. The trust
  instrument is valid e-signed; funding real property into the trust is
  a separate notarized, recordable deed and is stated in the document,
  not e-signed here.

  Background:
    Given a fresh Navigator app with the canonical templates seeded
    And a trust notation for the settlor "Capricorn" <capricorn@example.com>

  Scenario: Walking the trust questionnaire sends it for signature through the generalized path
    When the settlor walks the trust questionnaire:
      | value                     |
      | Capricorn                 |
      | The family home and a 401k|
    Then the final response status is 200
    And the trust notation workflow state is "sent_for_signature__pending"
    And the trust notation has a signature request id
    And the rendered trust names the trustee "Capricorn"
    And the rendered trust states the real-property notarization caveat

  Scenario: The trust workflow mirrors the retainer's signed shape
    Then the trusts__nevada workflow routes:
      | from                     | condition          | to                       |
      | BEGIN                    | intake_submitted   | intake_persisted__trustee|
      | intake_persisted__trustee| trust_rendered     | staff_review             |
      | staff_review             | approved           | document_open__trust_pdf |
      | document_open__trust_pdf | pdf_persisted      | sent_for_signature__pending|
      | sent_for_signature__pending | signature_received | END                   |
      | sent_for_signature__pending | signature_declined | END                   |
