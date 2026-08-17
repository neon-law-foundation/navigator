---
agent_workflow: issue-triage
model: reasoning-model
effort: high
max_turns: 12
---

Read the issue from its opening body through every comment. Ground the request in the glossary, narrowest relevant
documentation, current source, and covering tests. Do not edit source files, tests, documentation, or configuration.
Return exactly one JSON object with one non-empty `plan_markdown` field. Its Markdown value must be a structured,
test-driven implementation plan with the exact blast-radius files and the evidence that supports each step.
