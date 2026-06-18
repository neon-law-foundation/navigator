# Using the Navigator to Rapidly Solve Legal Outcomes

Lawyers are signing more documents in less time than ever, and a dependable way to keep that work correct is a harness —
a deterministic checklist, applied every time, that catches the things you already know to check (choice of law,
privilege, confidentiality, active voice, inclusive language) before you sign. In this workshop you will use the
Navigator through Gemini's "Add AIDA" connector — no installation, no command line — to build, ground, and deliver a
real deed of sale for a sample real-estate-purchase matter, with a notarization step the paralegal can run after you
leave. By the end of class you will have one notation you built and a three-minute demo you can repeat at your firm on
Monday. The steps below walk the whole loop end to end.

## Learning objectives

Each objective is tagged with the Bloom verb it exercises (the [Anderson & Krathwohl 2001
revision](https://en.wikipedia.org/wiki/Bloom%27s_taxonomy)). The lawyer is always the actor; Navigator is the
instrument.

- **Remember** — name the four Navigator nouns (Project, Template, Notation, Workflow) and locate each in the workspace
  glossary.
- **Understand** — explain in one sentence why every glossary noun is a database table, and why that makes Navigator's
  output deterministic.
- **Apply** — add AIDA to your Gemini workspace and issue a tool call that creates a new Project for a sample
  real-estate purchase.
- **Apply** — write a markdown template for a deed of sale with one `{{client_name}}` placeholder and bind it as a
  notation.
- **Analyze** — run the notation through the transactional checklist (choice of law, privilege, confidentiality, active
  voice, inclusive language) and identify which checks the draft passes and which it does not.
- **Evaluate** — review a peer's notation and propose one kaizen improvement — a single checklist item, glossary term,
  or template clause that would make the next draft faster or more correct.
- **Create** — advance the notation through its notarization workflow step and deliver a three-minute demo of the deed
  your lawyer-self would sign on Monday.

## The running matter

To keep everyone's example aligned, the class works one matter together:

- **Project** — *Henderson Bungalow Purchase*
- **Buyer** — *Virgo* (this becomes the value bound to `{{client_name}}`)
- **Property** — a single-family residence in Henderson, NV
- **Workflow step the class will run** — `notarization_pending` → `notarized` → `signed`

The same cast appears in the deed template, the cucumber test that grounds the workshop, and your final three-minute
demo. Three places, one cast, no surprise.

## How Navigator works

The entire secret of Navigator is this: it is a harness that grounds your LLM output in a deterministic, shared set of
glossary definitions, which are backed by database tables. The lawyer agrees, once, on what a `Notation` is, what a
`Project` is, what a `Workflow` step is — and from that point on, every drafting interaction speaks that same
vocabulary. The same nouns appear in the template you write, the questionnaire the client answers, the workflow that
advances the document toward signature, and the audit log your malpractice carrier will eventually read. No room for the
model to invent new categories of work.

Those Bloom rungs map one-to-one onto the noun ladder Navigator runs on:

1. **Project** — the matter ("Henderson Bungalow Purchase").
2. **Template** — a markdown blueprint with `{{placeholders}}` and a workflow declaration.
3. **Notation** — a Template come to life: one Person bound to one Template inside one Project, advancing through a
   workflow.
4. **Workflow** — the state machine the Notation walks
   (`draft → staff_review → notarization_pending → notarized → signed`).
5. **Signed** — the lawyer's own work product. Navigator does not sign anything; it makes it faster and more correct for
   *you* to sign.

When you have walked all five rungs once, you have done the entire Navigator loop. That is the workshop.

## Install (no install)

The class uses Gemini's "Add AIDA" connector. Open your Gemini workspace, click **Add connector**, paste the workshop's
connector URL (the instructor will display it), authenticate with your firm Google account, and confirm. Total time:
about ninety seconds. There is no local install, no CLI to configure, no MCP server to run yourself. The sandbox
environment that backs the connector is pre-provisioned for the class — your Project, your Template, and your Notation
all land in your isolated tenant. They will still be there after class so you can revisit and revise.

## Tool calls are just prompts with specific words

Once the AIDA connector is added, every "tool call" you make is just a regular Gemini prompt with one or two specific
words that route it through Navigator instead of the open web. Try one first:

> *"AIDA, create a project named Henderson Bungalow Purchase."*

Gemini will route the request through AIDA's `create_project` skill; AIDA writes a row in the `projects` table; the
response card shows you the new Project ID. You can interleave grounded calls (`AIDA, list my templates`) with open
searches (`look up NRS Chapter 111`) in the same chat. The difference is that the AIDA-routed calls return *rows*, not
paragraphs — that is the determinism we are after.

## Build the template

Write a small markdown template for a deed of sale. The minimum body for the class:

```markdown
# Deed of Sale

This Deed is made between {{client_name}} ("Buyer") and the named Seller for the property described
herein. Choice of law: Nevada. Buyer's signature must be acknowledged by a Nevada notary public under
Nevada's Uniform Law on Notarial Acts (NRS 240.161 to 240.169).

Buyer: ______________________
Date:  ______________________
```

The deed body leans on two Nevada statutes worth knowing by name: a conveyance is made by deed under NRS
[111.105](https://www.leg.state.nv.us/NRS/NRS-111.html#NRS111Sec105), and the buyer's signature is acknowledged by a
notary under Nevada's [Uniform Law on Notarial Acts](https://www.leg.state.nv.us/NRS/NRS-240.html).

The Foundation's free [statutes mirror](/statutes) republishes the practice-relevant NRS chapters as reference-only
verbatim text — read them without leaving the workshop, refreshed weekly. (Chapters 111 and 240 above aren't in that
curated set yet, so those links go to the official source.)

Then ask AIDA to bind the template as a notation for your Project, supplying `Virgo` as the value for `{{client_name}}`.
AIDA will return the rendered deed with the placeholder substituted and a workflow state of `draft`.

## Run the transactional checklist

Navigator's transactional checklist is the same list you already run in your head when you read a draft: choice of law,
privilege, confidentiality, active voice, inclusive language. Ask AIDA:

> *"AIDA, run the transactional checklist on this notation and tell me what fails."*

You will get a row per check, pass or fail. This is the **Analyze** rung. Fix what fails by editing the template; bind a
new notation; re-run.

## Kaizen — share what you found

Kaizen (改善, [Imai 1986](https://en.wikipedia.org/wiki/Kaizen)) is the Japanese principle of small, iterative
improvement. Programmers have been taught it for decades; Navigator is designed for the same loop at the legal-drafting
layer. Each pass through the checklist that surfaces a new failure is one kaizen step — add the clause, add the glossary
term, add the check, repeat. You are encouraged to take pieces of Navigator back to your firm, keep using it, and share
what you learn with the next lawyer.

## When AIDA asks before she acts

Every AIDA call in this class runs over the same A2A connector Gemini Enterprise uses, and that surface has one rule
worth naming out loud: **reads run, writes wait.** Looking something up — `list my templates`, `show me this notation` —
happens immediately. Anything that *acts* in a client-facing way — sending an email, routing a deed for signature —
pauses and asks you first:

> *"Authorize this action? AIDA wants to Send Welcome Email for Virgo… Reply yes to authorize, or no to cancel."*

Reply `yes` and it runs; reply `no` and nothing happens. That pause is not a limitation — it is the supervision a
licensed attorney owes any non-lawyer assistant (ABA Model Rule 5.3), and it is the same gate behind "the deed is not
signed until you, the attorney, explicitly advance the workflow." AIDA proposes; you authorize.

If a call fails — a bad jurisdiction code, a malformed import — the chat now tells you *why*, in plain text, so you can
fix it and re-run rather than staring at a blank "it didn't work." The full behavior is documented in [AIDA over A2A —
confirmations and errors](/docs/aida-a2a-interaction).

## Notarize and demo

Advance the notation: `draft → staff_review → notarization_pending`. The notarization step is a real workflow state;
AIDA emits the cue your paralegal needs to schedule the notary. Once notarized, the workflow advances to `signed`. **The
deed is not signed until you, the attorney, explicitly advance the workflow.** Navigator will never sign anything for
you.

For the three-minute demo, walk us through:

1. The matter ("Henderson Bungalow Purchase, buyer Virgo").
2. The template you wrote (show the markdown).
3. The notation you bound (show the rendered deed).
4. The one checklist failure you found and fixed.
5. The workflow advance to `notarized`.

That is enough. Three minutes is plenty — clarity over coverage.

## Why this matters

A harness — a deterministic checklist applied every time — is how routine legal work gets cheap enough to reach the
people priced out of it today. The same loop that lets us produce a deed for $200 lets a legal-aid clinic produce
twenty. That is the access-to-justice fight, and these steps equip you to join it. Read the [Foundation
mission](/foundation/mission) for why it matters.

## Run your own — and drive it from the command line

This workshop used Navigator through Gemini's "Add AIDA" connector — no installation, no command line. When you are
ready to run your **own** Navigator, the [Deploy the Navigator](/foundation/workshops/navigator/deploy) workshop stands
up the same stack on your own Google Cloud project. Once it is live you do not need a browser to drive it: the
`navigator` CLI logs in to *your* installation the way `gcloud auth login` does. `navigator login --host <your-host>`
mints a short-lived token, after which `navigator matter open`, `navigator retainer approve`, and `navigator notation
status` run the same matter flow you did here, from your terminal. The host is whatever you named your deployment, so
the one CLI drives every instance you stand up.

## Form a Nevada LLC from the command line

The same CLI forms a real Nevada LLC end to end — no browser — once you have logged in. You open a questionnaire-driven
matter, answer the formation questions at the terminal, and download the **filled official Nevada Secretary of State
packet**: the same artifact a browser walk produces, the one you review before the staff-gated filing.

```bash
navigator login https://your-firm.example
navigator matter open --template onboarding__nest --client-email libra@example.com
navigator intake answer <notation-id>
navigator notation status <notation-id>
navigator notation approve <notation-id>
navigator notation document <notation-id> --out llc.pdf
```

`matter open` starts the `onboarding__nest` matter and prints its notation id. `intake answer` then walks the formation
questionnaire one question at a time — the entity name, the registered agent, whether the company is member-managed or
manager-managed, and the managing members entered row by row (a blank name ends the list). Answer it interactively, or
script it with repeated `--answer` and `--person` flags. `notation status` reports the workflow state and whether the
packet has been rendered; `notation approve` renders and parks the filled packet for your review; `notation document`
writes that PDF to `--out`. AIDA fills the state's official form from the answers — it never invents one — and the
matter ends at the same staff-gated `filing__nv_sos` step a browser walk reaches: **you file with the Secretary of
State; Navigator never files for you.**

This whole command-line round-trip is covered by an automated test that drives the real `navigator` binary and checks
the downloaded bytes are the official packet carrying the founder's answers. The pipeline behind the fill — vendoring
the canonical form, mapping answers to its fields, and the staff-gated filing that ends it — is laid out step by step in
[Government forms: vendor, map, fill, file](/docs/gov-forms).

## Share what you built

When your three-minute demo is finished, send the markdown of your template — and the one kaizen improvement you found —
to [support@neonlaw.org](mailto:support@neonlaw.org?subject=Workshop+demo). Every template a lawyer contributes raises
the floor of competence for the next lawyer who joins.
