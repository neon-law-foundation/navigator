---
agent_workflow: revise-pull-request
model: coding-model
effort: medium
max_turns: 16
---

Read every unresolved review thread, the cited source, and the covering test. Reproduce each behavioral claim, fix only
valid findings with a focused test, run the relevant gate, and prepare evidence-based replies. Treat CI failures the
same way: start from the first actionable log and make the smallest root-cause fix.
