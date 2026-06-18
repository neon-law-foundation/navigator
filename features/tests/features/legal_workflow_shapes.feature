Feature: Bundled-template workflow shapes (LLC, trust, will)

  The retainer is BDD-tested end-to-end via the walker; the other
  three bundled templates lock down their state-machine shapes here
  instead. The `workflow_integrity` workspace test asserts a small
  set of generic invariants (BEGIN present, END reachable, every
  transition target exists, every workflow prefix resolves to a
  `StepKind`); these scenarios pin the *exact* transitions per
  template so an accidental reshape — adding a witness step to the
  LLC, for example — surfaces as a named failing scenario.

  The Nevada trust now rides the e-signature engine (its workflow
  branches on named conditions rather than the linear `_` chain these
  shape-locks walk), so its workflow shape and signed send path are
  pinned in `trust_esignature.feature` instead. Its questionnaire shape
  stays here.

  Each template also gets one rejection scenario: a hand-mutilated
  copy with `END:` excised must fail to parse with `MissingEnd`, so
  the parser's guardrails stay load-bearing.

  Scenario: California LLC questionnaire walks company → office → members → END
    Given the bundled template "llc/california.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from             | to                |
      | BEGIN            | company_name      |
      | company_name     | principal_office  |
      | principal_office | member_list       |
      | member_list      | END               |

  Scenario: California LLC workflow walks member signatures → staff review → END
    Given the bundled template "llc/california.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from              | to                |
      | BEGIN             | member_signatures |
      | member_signatures | staff_review      |
      | staff_review      | END               |
    And every workflow state resolves to a StepKind

  Scenario: California LLC template with END stripped fails to parse
    Given the bundled template "llc/california.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error

  Scenario: Nevada trust questionnaire walks trustee → property → END
    Given the bundled template "trust/nevada.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from          | to             |
      | BEGIN         | trustee_name   |
      | trustee_name  | trust_property |
      | trust_property| END            |

  Scenario: Nevada trust template with END stripped fails to parse
    Given the bundled template "trust/nevada.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error

  Scenario: Simple will questionnaire walks testator → executor → residuary → END
    Given the bundled template "will/simple.md"
    Then the questionnaire transitions, in BEGIN-first order, are:
      | from                  | to                    |
      | BEGIN                 | testator_name         |
      | testator_name         | executor_name         |
      | executor_name         | residuary_beneficiary |
      | residuary_beneficiary | END                   |

  Scenario: Simple will workflow walks testator signature → witnesses → staff review → notarization → END
    Given the bundled template "will/simple.md"
    Then the workflow transitions, in BEGIN-first order, are:
      | from               | to                 |
      | BEGIN              | testator_signature |
      | testator_signature | witnesses          |
      | witnesses          | staff_review       |
      | staff_review       | notarization       |
      | notarization       | END                |
    And every workflow state resolves to a StepKind

  Scenario: Simple will template with END stripped fails to parse
    Given the bundled template "will/simple.md" with the workflow END declaration removed
    Then parsing the workflow spec returns a MissingEnd error
