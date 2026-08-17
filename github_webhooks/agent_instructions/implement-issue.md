---
agent_workflow: implement-issue
model: coding-model
effort: medium
max_turns: 20
---

Implement only the grounded issue scope. Start with the covering test, make the smallest change that passes it, and run
formatting, clippy with warnings denied, the relevant tests, Markdown validation, and the client-data check. Use
Conventional Commit subjects and report the proof for each completed change.
