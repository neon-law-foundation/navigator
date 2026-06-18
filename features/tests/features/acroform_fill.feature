Feature: Fill a fillable government form, attorney-review-ready
  A finished questionnaire can populate a blank fillable government form
  (an AcroForm PDF — Nevada SoS articles, IRS 990) and produce an
  attorney-review-ready document. The fill runs as a worker-dispatched
  `document_open__<form>` step (the same seam the retainer PDF uses) and
  the output is NEVER auto-filed: the workflow spec parks it at
  `staff_review` before any filing step, enforced by the review gate.

  Scenario: A document_open__<form> step fills the form and the values round-trip
    Given a blank fillable "nv_articles" form is stored with fields "entity_name", "registered_agent"
    When the worker fills it for "Neon Law LLC" with agent "Jane Doe"
    Then the stored form's "entity_name" reads "Neon Law LLC"
    And the stored form's "registered_agent" reads "Jane Doe"

  Scenario: The form workflow cannot reach a filing step without staff_review
    Given the nv_articles workflow spec
    Then the staff_review gate holds between fill and filing
