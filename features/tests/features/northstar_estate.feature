Feature: Northstar estate, end to end

  Neon Law Northstar is an estate plan, priced bespoke per client. This
  follows Capricorn, an elder planning their legacy, and one Neon Law
  attorney across the whole arc: the matter is opened, the attorney drafts
  the will, Capricorn reads it on the first-party review surface and leaves
  a comment, the attorney resolves it, and the firm signs the closing letter
  to close the matter.

  Closing the matter raises no money. Accounting originates in Xero, where
  lawyers agree the price and raise the invoice themselves, so the accounting
  seam must stay untouched across the whole arc.

  This is the cross-surface stitch the suite exists to prove: one matter
  touching the review_documents surface and the closing walker in a single
  representation.

  Background:
    Given a client named "Capricorn" <capricorn@example.com> planning their estate

  Scenario: Draft, client review, resolution, and close — with nothing invoiced
    When AIDA opens the estate matter and the attorney drafts the will
    Then Capricorn can read the will on the review surface
    When Capricorn leaves a comment on the draft
    Then the comment is recorded on the draft
    When the attorney resolves the comment
    Then the comment is resolved
    When the firm signs the closing letter to close the matter
    Then the matter is closed
    And the accounting seam was never called
