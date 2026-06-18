Feature: Inbound-email round trip, end to end

  The "headless Front" loop: a client emails support, the firm threads the
  message into a conversation, an attorney binds the thread to the client's
  matter with an `@link` command, and the attorney's reply relays back to
  the client — all without anyone leaving their inbox. This follows Aries,
  who has more than one open matter — so the engine can't auto-route the
  thread and the attorney picks the matter with `@link` — and the attorney
  who answers.

  Background:
    Given a client named "Aries" <aries@example.com> with two open matters
    And a staff member "staff@neonlaw.com"

  Scenario: A client email threads, binds to the matter, and the reply relays back
    When the client emails support asking about their filing
    Then a support conversation is opened for the client
    And the firm is notified with a reply-to thread token
    When the attorney replies "@link" to the thread and answers the client
    Then the conversation is bound to the client's matter
    And the attorney's answer is relayed back to the client
