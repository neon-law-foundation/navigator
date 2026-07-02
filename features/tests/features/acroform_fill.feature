Feature: Fill a fillable government form, attorney-review-ready
  A finished questionnaire can populate a blank fillable government form
  (an AcroForm PDF — Nevada SoS articles, IRS 990) and produce an
  attorney-review-ready document. The fill runs as a worker-dispatched
  `document_open__<form>` step (the same seam the retainer PDF uses) and
  the output is NEVER auto-filed: the workflow spec parks it at
  `staff_review` before any filing step, enforced by the review gate.

  Scenario: A document_open__<form> step fills the form and the values survive the flatten
    Given a blank fillable "nv_articles" form is stored with fields "entity_name", "registered_agent"
    When the worker fills it for "Neon Law LLC" with agent "Jane Doe"
    Then the flattened output carries "Neon Law LLC" as static text
    And the flattened output carries "Jane Doe" as static text

  Scenario: The form workflow cannot reach a filing step without staff_review
    Given the nv_articles workflow spec
    Then the staff_review gate holds between fill and filing
