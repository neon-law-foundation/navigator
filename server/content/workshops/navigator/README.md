# Using the Navigator Workshop

Lawyers are signing more documents in less time than ever, and a dependable way to keep that work correct is a harness —
a deterministic checklist, applied every time, that catches the things you already know to check. The hosted class uses
Gemini's "Add AIDA" connector. A local KIND rehearsal instead uses the Navigator browser surfaces: it proves the seeded
matter, the lawyer workbench, and the client portal before a presenter relies on an externally reachable Gemini setup.

## Intro

### Learning objectives

The lawyer is always the actor; Neon Law Navigator is the instrument. Each objective is tagged with its Bloom verb:

- **Remember** — name the four Neon Law Navigator nouns and locate each in the workspace glossary.
- **Understand** — explain why every glossary noun is a database table.
- **Apply** — open the seeded Henderson Project and bind its pre-seeded deed template as a notation in a configured AIDA
  environment.
- **Analyze** — validate a draft's notation structure and identify the lawyer's checklist findings.
- **Evaluate** — review a peer's notation and propose one kaizen improvement.
- **Create** — explain the notarization workflow step and deliver a three-minute demo from a configured workflow
  channel.

---

Each objective is tagged with the Bloom verb it exercises (the [Anderson & Krathwohl 2001
revision](https://en.wikipedia.org/wiki/Bloom%27s_taxonomy)). The lawyer is always the actor; Neon Law Navigator is the
instrument. In full: **Remember** — name the four Neon Law Navigator nouns (Project, Template, Notation, Workflow) and
locate each in the workspace glossary. **Understand** — explain in one sentence why every glossary noun is a database
table, and why that makes Navigator's output deterministic. **Apply** — use the seeded Henderson Project and deed
template in a configured AIDA environment. **Analyze** — distinguish structural notation diagnostics from the lawyer's
substantive checklist. **Evaluate** — review a peer's notation and propose one kaizen improvement. **Create** — explain
the configured, attorney-driven notarization transition without representing it as a local browser or generic AIDA tool.

### The running matter

The class works one matter together so every example aligns:

- **Project** — *Henderson Bungalow Purchase*
- **Buyer** — *Virgo* (the value bound to `{{client_name}}`)
- **Property** — a single-family residence in Henderson, NV
- **Workflow step** — `lawyer_review` → `notarization__pending` → `notarized` (complete)

---

To keep everyone's example aligned, the class works one matter together. The Project is *Henderson Bungalow Purchase*,
the buyer is *Virgo* (the value bound to `{{client_name}}`), and the property is a single-family residence in Henderson,
NV. The workflow step the class will run is `lawyer_review` → `notarization__pending` → `notarized` (complete). The same
cast appears in the deed template, the cucumber test that grounds the workshop, and your final three-minute demo. Three
places, one cast, no surprise.

### Who is in the room

This workshop is for **Lawyer** users of the application: licensed lawyers who work matters for clients. The client in
the class is Virgo; Virgo is the person the firm represents on the Henderson Bungalow Purchase matter. You will use the
lawyer workbench and AIDA to open the matter, bind the notation, review the checklist, and advance the workflow.

---

Navigator keeps the audience split precise because the authorization model is precise. `persons.role` has five stored
values in authority order (and anonymous is the public visitor with no row):

- `owner` — the system owner and highest tier. Owner inherits Admin and Lawyer authority; only Owner governs Owner.
- `admin` — a licensed lawyer with system administration authority. Admins manage installation-wide settings and can see
  every Project without per-Project assignment, but cannot manage Owner identities.
- `lawyer` — a person **licensed to practice law**. Lawyer users work assigned matters through `/lawyer` and AIDA; the
  legal workflow still records which lawyer advanced each step.
- `clerk` — a supervised **non-lawyer** worker. Clerk's `/clerk` surface provides a read-only list of supervised
  Projects and their lawyer DRI; Clerk receives no legal advice, approval, Git, MCP, or `/lawyer` authority by
  inheritance. Any upload or preparation task remains a narrow, supervised Project capability.
- `client` — a person represented on one or more matters. Clients use `/app/projects` to see reviewed documents,
  Engagements/Notations, invoices, signatures, and other client-facing matter surfaces.

Project participation is separate from role. A client sees a matter through the client lens because the Project records
them as a client participant. A Lawyer user sees a matter through the firm lens only when the Project has a firm-side
participation row for that lawyer; the `is_lawyer_dri` marker rides that same participation row, so naming the lawyer
DRI and recording their firm-side participation are one act. Clerk work begins with `/clerk`: the Clerk's own firm-side
participation and a disclosed Lawyer DRI are both required for its limited coordination view. That is why the
application can delegate preparation without turning a Clerk into a lawyer.

### Local KIND presenter rehearsal

Start a fresh worktree environment, source its descriptor, and run the host web process:

```bash
cargo run -p cli -- dev worktree-env up --path "$PWD"
set -a; source .devx/env; set +a
cargo run -p neon
```

Then browse to `$NAV_BASE_URL/auth/login`. Sign-in lands each tier on its own home — a firm tier on the `/app/team`
home, a client on `/app/projects`. The stock local accounts are deliberately different lenses on the same seeded matter:

- `lawyer@neonlaw.com` / `password` is the Lawyer presenter. Sign-in lands on the `/app/team` home; open `/app/projects`
  to see *Henderson Bungalow Purchase* through Lawyer's firm-side `paralegal` participation — the workbench lens.
- `client@neonlaw.com` / `password` is the client presenter. Sign-in lands on `/app/projects`, which lists *Henderson
  Bungalow Purchase* through the client account's `client` participation — the client lens. Its detail has no seeded
  client Documents, Engagements, invoices, or review documents; those surfaces remain empty until a later exercise
  creates and releases them.

`lawyer` and `client` are system roles; `paralegal` and `client` above are per-Project participation descriptions. Do
not use the now-unnecessary `navigator dev grant-lawyer` step for this fresh seed: it remains harmless for the browser
harness, but it does not add project membership. Re-login after changing any role or participation.

---

Rehearse this before class, not live: the first `worktree-env up` provisions a KIND cluster and can take several
minutes, so have the seeded matter already open when you share your screen. A Lawyer lands on the `/app/team` home and
reaches a matter through the `/app/projects` workbench lens. Keep both the `lawyer` and `client` browser sessions signed
in ahead of time so you can switch lenses without re-authenticating in front of the room.

### How Neon Law Navigator works

Neon Law Navigator grounds your LLM output in a deterministic, shared glossary backed by database tables. The noun
ladder:

1. **Project** — the matter ("Henderson Bungalow Purchase").
2. **Template** — a markdown blueprint with `{{placeholders}}` and a workflow declaration.
3. **Notation** — one Person bound to one Template inside one Project, advancing through a workflow.
4. **Workflow** — the state machine the Notation walks.
5. **Signed** — the lawyer's own work product.

---

The entire secret of Neon Law Navigator is this: it is a harness that grounds your LLM output in a deterministic, shared
set of glossary definitions, which are backed by database tables. The lawyer agrees, once, on what a `Notation` is, what
a `Project` is, what a `Workflow` step is — and from that point on, every drafting interaction speaks that same
vocabulary. The same nouns appear in the template you write, the questionnaire the client answers, the workflow that
advances the document toward signature, and the audit log your malpractice carrier will eventually read. No room for the
model to invent new categories of work.

Those Bloom rungs map one-to-one onto the noun ladder Neon Law Navigator runs on. The **Project** is the matter. The
**Template** is a markdown blueprint with `{{placeholders}}` and a workflow declaration. The **Notation** is a Template
come to life: one Person bound to one Template inside one Project, advancing through a workflow. The **Workflow** is the
state machine the Notation walks (`lawyer_review → notarization__pending → notarized → complete`). And **Signed** is the
lawyer's own work product — Neon Law Navigator does not sign anything; it makes it faster and more correct for *you* to
sign. When you have walked all five rungs once, you have done the entire Neon Law Navigator loop. That is the workshop.

### Matter files: portal for clients, workbench for lawyers

Every Project also has a matter file surface. Clients see it as **Documents**, **Engagements**, **Invoices**, and other
plain-English portal views. If a person is added to a Project, Navigator can show them the client-facing files for that
matter; if they are not added, those files do not exist from their portal's point of view.

Lawyer users have the firm workbench. Assigned lawyers work the Project through `/lawyer`, and lawyer-tier users who
prefer an editor can mirror their matters onto their own machine with `navigator site sync` — one folder per matter
under `~/Projects`. Admins are lawyer-tier users with installation-wide authority, so they can reach the same firm
workbench without per-Project assignment. Clients never need that layer and never see the private GCS bucket behind the
files.

---

This slide is where you separate "client can see their matter file" from "lawyer can work the matter." In Navigator,
client access is portal-native: Documents, Engagements, Invoices, comments, signatures, and review surfaces filtered by
the Project participant list. Lawyer access adds the firm workbench, plus an on-disk mirror for attorneys and paralegals
who would rather work in an editor than a browser. The private bucket is an implementation detail behind both surfaces,
not an access control panel we hand to clients.

Say plainly what the mirror is and is not: it is a working copy scoped to your own participation, and the site remains
the record. Nothing about having a folder on your laptop changes who can see a matter — the same participation ledger
decides both surfaces.

### The Shared Drive and Project repository map

The firm's **Projects** Shared Drive is the production matter-file root. Its top-level folders are Navigator Project
codes, not display titles: a matter whose code is `henderson-bungalow-purchase` lives at
`Projects/henderson-bungalow-purchase/`. A code uses only lowercase letters, digits, and single hyphens; Navigator
requires it when a Project opens because the name is an equality check, not a display-name normalization.

The same convention is deployment-owned rather than guessed from a laptop or a repository:

| Deployment | Shared Drive root | Organization |
| --- | --- | --- |
| Neon Law production | `Projects` | `neon-law` |
| Neon Law staging | `Staging Projects` | `neon-law` |
| Neon Law Foundation | `NLF Projects` | `neon-law-foundation` |

The organization is configuration rather than a name in Navigator's source, and one string means two different things
across the two vocabularies: the organization `neon-law` is *staging*, while the GCP project `neon-law` is *production*.
The organizations are named for the entities and the GCP projects for the deployments.

A Project has one repository and one portal, and both are the code. That matter's source lives at
`neon-law/henderson-bungalow-purchase`, holding its notation templates under `templates/` and its client portal under
`portal/`, and Navigator serves that portal at `/app/projects/henderson-bungalow-purchase/portal/` — the repository name
plus one literal segment. Nothing is composed, so nothing has to be parsed back apart, and there is no manifest anywhere
restating a name the repository already carries.

Drive holds the firm's legal working files and Navigator holds the matter record and asset provenance. Project
repositories hold source only: never client uploads, answers, generated legal documents, secrets, dependencies, or build
output. When CI publishes approved Project-scoped template or application output to Drive, it resolves the matter folder
from Navigator rather than from a repository-supplied folder ID, writes one way, and records the publication for audit.
A hand edit to that published output is drift, not a source change. Project participation grants Navigator and
deployed-application access; it never grants GitHub Enterprise access.

---

Treat the Shared Drive name as an operational contract: the top-level folder is the Project code, so a lawyer can locate
a matter without translating a client-facing display title. The three deployment roots keep production, staging, and
Foundation material separate while preserving the same path grammar.

Stress the source boundary. Repositories describe templates and applications; Drive receives approved publication
output; Navigator remains the record of the matter and of what was published. A Drive folder is not a shortcut around
Project participation or source review.

## Build the Notation

### Install (no install)

The class uses Gemini's "Add AIDA" connector. About ninety seconds:

- Open your Gemini workspace and click **Add connector**. Paste the workshop's connector URL (the instructor will
  display it). Authenticate with your firm Google account, and confirm.

---

There is no local install, no CLI to configure, no MCP server to run yourself. The sandbox environment that backs the
connector is pre-provisioned for the class — your Project, your Template, and your Notation all land in your isolated
tenant. They will still be there after class so you can revisit and revise.

This is a hosted-connector instruction, not a local KIND instruction. Gemini must be able to reach the configured A2A
endpoint and complete its Google/OIDC setup; a worktree's `localhost` URL is only for the browser rehearsal above and
must not be pasted into Gemini's connector dialog.

### Sign in as yourself

The connector URL is not access to the firm's matters. AIDA acts as the person who signed in:

- Gemini sends Navigator the Google OAuth token for the firm account you selected; Navigator verifies it, then looks
  that email up in its own people record.
- Only a person recorded as `lawyer` or `admin` can use AIDA's matter tools. You cannot gain that role by asking AIDA.
- Denied during class? Sign in with the lawyer account the instructor provided, or ask them to check your role.

---

Navigator verifies that the token came from the registered connector, that its email is verified, and — on a production
firm installation — that it belongs to the permitted Workspace domain. A client-tier or unseeded account is denied even
with a valid firm Google sign-in; clients use the portal rather than the lawyer workbench. Navigator carries that
verified identity with the request, so AIDA acts as the lawyer who started the conversation — not as an anonymous
chatbot. For a client-facing action, the same lawyer-side principal must explicitly confirm the proposed action, and
Navigator records that decision in the audit trail.

### Tool calls are just prompts with specific words

Every "tool call" is a regular Gemini prompt with one or two words that route it through Neon Law Navigator. Try one:

> *"AIDA, list my projects."*

---

Once the AIDA connector is added, Gemini can route that request through AIDA's `list_projects` skill. The currently
advertised catalog is the source of truth: `aida_create_notation` binds an **existing** template to an **existing**
Project, while `aida_validate_notation` lints supplied Markdown. Neither tool creates or imports a template, and there
is no generic AIDA workflow-advance tool. The local dev seed supplies `real_estate__deed_of_sale` precisely so the
Henderson rehearsal has an existing template to bind.

### Build the template

Write a small markdown template for a deed of sale. The minimum body for the class:

```markdown
# Deed of Sale

This Deed is made between {{client_name}} ("Buyer") and the named Seller for the property described
herein. Choice of law: Nevada. Buyer's signature must be acknowledged by a Nevada notary public under
Nevada's Uniform Law on Notarial Acts (NRS 240.161 to 240.169).

Buyer: ______________________
Date:  ______________________
```

---

The deed body leans on two Nevada statutes worth knowing by name: a conveyance is made by deed under NRS
[111.105](https://www.leg.state.nv.us/NRS/NRS-111.html#NRS111Sec105), and the buyer's signature is acknowledged by a
notary under Nevada's [Uniform Law on Notarial Acts](https://www.leg.state.nv.us/NRS/NRS-240.html).

For a configured AIDA session, bind the pre-seeded `real_estate__deed_of_sale` template to the existing Henderson
Project by its ID. The create-notation tool returns the notation and its next state; it does not persist Markdown you
typed in Gemini or promise a rendered, placeholder-substituted deed. Use the browser rehearsal to show the seeded matter
and the configured connector environment for the binding call.

### Run the transactional checklist

The checklist is the same list you run in your head: choice of law, privilege, confidentiality, active voice, inclusive
language. `aida_validate_notation` can report structural notation diagnostics for supplied Markdown; it is not a named
transactional-checklist service and it does not save the draft.

> *"AIDA, validate this notation Markdown and show the diagnostics."*

---

Record the lawyer's substantive checklist findings separately. This is the **Analyze** rung: fix the draft in its real
authoring repository or other supported template-import path, bind a new notation, then validate again.

### Kaizen — share what you found

Kaizen (改善, [Imai 1986](https://en.wikipedia.org/wiki/Kaizen)) is the principle of small, iterative improvement. Each
checklist pass that surfaces a failure is one kaizen step.

---

Programmers have been taught kaizen for decades; Neon Law Navigator is designed for the same loop at the legal-drafting
layer. Each pass through the checklist that surfaces a new failure is one kaizen step — add the clause, add the glossary
term, add the check, repeat. You are encouraged to take pieces of Neon Law Navigator back to your firm, keep using it,
and share what you learn with the next lawyer.

## Keep the Attorney in Control

### When AIDA asks before she acts

One rule on the A2A connector: **reads run, writes wait.** A write pauses and asks first:

> *"Authorize this action? AIDA wants to Send Welcome Email for Virgo… Reply yes to authorize, or no to cancel."*

---

Every AIDA call in this class runs over the same A2A connector Gemini Enterprise uses. Looking something up — say `list
my projects` — happens immediately. Anything that *acts* in a client-facing way — sending an email, routing a deed for
signature — pauses and asks you first. Reply `yes` and it runs; reply `no` and nothing happens. That pause is not a
limitation — it is the supervision a licensed attorney owes any non-lawyer assistant (ABA Model Rule 5.3), and it is the
same gate behind "the deed is not signed until you, the attorney, explicitly advance the workflow." AIDA proposes; you
authorize.

If a call fails — a bad jurisdiction code, a malformed import — the chat now tells you *why*, in plain text, so you can
fix it and re-run rather than staring at a blank "it didn't work." The full behavior is documented in [AIDA over A2A —
confirmations and errors](/docs/aida-a2a-interaction).

### When an answer is wrong, send it back — don't start over

Review is not only approve or decline. A third choice keeps the matter moving:

- Flag the specific answers that are wrong, add a note, and send it back. Only those get re-collected — by the client
  from their portal, or by your paralegal on their behalf — and it returns to your desk for review.

---

Every matter parks at `lawyer_review` before anything binds — that is the gate you already know. What is new is what
happens when the draft is *almost* right. You do not have to approve a bad answer, and you do not have to end the matter
to fix one. You **request changes**: check the answers that are wrong — the misspelled name, the wrong entity type —
write a one-line note, and the matter moves to a re-ask state instead of dead-ending. The client can correct their own
answers, or a paralegal can correct them on the client's behalf; either way it comes back to `lawyer_review` and you
review the corrected draft. `Decline` is still there as its own action for a matter that should genuinely end — but "an
answer was wrong" is no longer the end of the road. It mirrors how you already work: a redline is not a rejection of the
whole engagement, it is a note on the one clause that needs fixing.

### Answers and questions are two different things

You are correcting an *answer*, not re-opening the *questionnaire*:

- Only the answers you flagged are re-collected. Every other answer stays as it was, and the matter never re-walks the
  whole intake.
- The paper stays pinned. Correcting an answer does not swap the template version the matter opened on — the bytes you
  review are the bytes the client signs.

---

This is the distinction that keeps the audit trail honest. A **question** is part of the template — the immutable paper
the matter opened on, pinned by version at the moment you opened it. An **answer** is the client's response, which you
can correct through the matter's life without disturbing the paper. So when you send an answer back and it is
re-collected, three things hold at once: only the flagged answers change, the template version stays exactly what it
was, and the journal records who corrected what, when, against which version. From the command line the same loop is:

```bash
navigator site notation request-changes <id> --question person__client --note "confirm the spelling"
navigator site notation update <id> --answer person__client="Libra Jones"
navigator site notation approve <id>
```

The matter you approve is the matter you reviewed — nothing re-rendered behind your back.

### The conflict check runs before every new matter

When you create a Project, Navigator runs a conflict check **first** — before the matter exists:

> *"Conflict check flagged this matter for review: shares a party with a current client's matter. Confirm you have
> reviewed these findings and are authorized to proceed."*

---

Opening a matter is a write, so — like every write in this class — it pauses. But this pause is doing conflicts work.
Navigator builds a graph from the firm's relationships (who manages which entity, who is adverse to whom) and walks it
out from your proposed client and entity to see whether the new matter touches a client the firm already serves. The
check is **advisory to clear, authoritative to block**: a confident, direct adverse link to a current client *blocks*
the open, and a softer entanglement — a shared entity, a recorded disclosure — is *flagged for you to acknowledge*.

The lawyer is still the actor. The graph can **raise** a conflict; only you can **clear** one — by reviewing the finding
and acknowledging it, which Navigator records to the relationship log as your decision. It is a harness for the conflict
review you already owe under the Rules of Professional Conduct (1.7 / 1.9, imputed firm-wide by 1.10), not a substitute
for your judgment, and it does not promise to surface every conflict — your independent check still governs. The same
gate runs on every path that opens a matter: the portal, the AIDA tool call, and the command line.

## Complete the Matter

### Notarize and demo

In a configured workflow channel, advance the notation: `lawyer_review → notarization__pending → notarized` (complete).
The stock local KIND rehearsal does not include a Gemini connector or a configured reply-email channel, so do not
present this as a browser-only local action. For the three-minute demo:

At `lawyer_review`, reply to the conversation's token address with these two command lines:

```text
@link <notation-id>
@approve
```

`@link` connects the conversation to the Notation. `@approve` then fires the workflow's `approved` condition.

1. The matter ("Henderson Bungalow Purchase, buyer Virgo").
2. The template you wrote (show the markdown).
3. The notation you bound (show its returned notation record and the source template).
4. The one checklist failure you found and fixed.
5. The workflow advance to `notarized`.

---

The notarization step is a real workflow state, and `notarized` is where the seeded workflow completes. Neon Law
Navigator advances the notation no further on its own. **The deed is signed only when you, the attorney, take that step
yourself through a configured supported channel.** Neon Law Navigator will never sign anything for you. Three minutes is
plenty for the demo — clarity over coverage.

The email command channel has a narrow trust boundary. The sender's email must resolve to a Person whose `persons.role`
is `lawyer` or `admin`; when `NAVIGATOR_DKIM_REQUIRE_DOMAIN` is configured, the reply must also pass DKIM for that firm
domain. Project participation descriptions such as `attorney` and `paralegal` are separate from this authorization
decision. Navigator strips the `@link` and `@approve` lines before any client relay, although prose in the same reply
may relay. The resulting workflow event records the sending lawyer Person as the actor—never the client or a generic
fallback.

### Why this matters

A harness — a deterministic checklist applied every time — is how routine legal work gets cheap enough to reach the
people priced out of it today.

---

The same loop that lets us produce a deed for $200 lets a legal-aid clinic produce twenty. That is the access-to-justice
fight, and these steps equip you to join it. Read the [Foundation mission](/foundation/mission) for why it matters.

## Take It to the CLI

### Run your own — and drive it from the command line

This workshop used the "Add AIDA" connector. When you are ready to run your **own** Neon Law Navigator, the [Deploy the
Neon Law Navigator](/workshops/deploy-the-navigator) workshop stands up the same stack on your own Google Cloud project
— and once it is live, the `navigator` CLI drives it from your terminal:

```bash
navigator site login --host <your-host>   # mints a short-lived token
```

---

Once your installation is live you do not need a browser to drive it: the `navigator` CLI logs in to *your* installation
like `gcloud auth login`. `navigator site login --host <your-host>` mints a short-lived token, and after that the
`navigator site notation create`, `navigator site retainer approve`, and `navigator site notation status` commands run
the same matter flow here, from your terminal. The host is whatever you named your deployment, so the one CLI drives
every instance you stand up.

### Put your matters on your own machine

Two commands mirror every matter you are on into a folder tree you can open in an editor:

```bash
navigator site login --host <your-host>   # mints a short-lived token
navigator site sync                       # one folder per matter under ~/Projects
```

You get `~/Projects/<matter-code>/` for each matter, a `README.md` card in each, and a `CLAUDE.md` at the root.

---

`sync` is a read. It asks your installation for the matters you participate in — the same participation-scoped list the
lawyer workbench shows you — and writes that list to disk. It cannot show you a matter the site would not, because it
does no filtering of its own: the server answers, and sync writes down the answer.

Three properties are worth stating out loud in the room, because they are what make it safe to re-run:

- **Sync owns three files.** `CLAUDE.md` and `AGENTS.md` at the root, and a `README.md` in each matter folder. It
  rewrites those on every run and touches nothing else. Your own drafts, notes, and scratch files stay exactly as you
  left them.
- **Sync never deletes.** When a matter closes or your participation ends, its folder stops being refreshed and is
  reported to you — not removed. Deciding what happens to a folder that may hold your own work is your call.
- **The site is still the record.** The folder is a working copy. The matter's workflow step, its notations, its filed
  documents, and its audit trail live on your installation, and each `README.md` links straight to that matter's
  workbench.

Re-run `navigator site sync` whenever you want the tree current. It rewrites only what actually changed.

### Open a matter folder in Claude

The root `CLAUDE.md` is the instruction every agent session inherits. Open one matter folder and start work:

```bash
cd ~/Projects/<matter-code>
claude
```

Claude reads `~/Projects/CLAUDE.md` on the way down, so it arrives already knowing this is client material.

---

Agent tools read their guidance files up the directory tree, which is why the guide sits at `~/Projects` rather than
being copied into every matter folder: write it once, and it governs every matter opened beneath it. `AGENTS.md` is the
same text under the filename other tools look for, so the rules do not depend on which assistant an attorney happens to
use.

What that guide establishes, before any prompt is typed: everything in the tree is confidential under Rule 1.6 and
mostly privileged; matter content does not go into tools the firm has not approved for client data, and does not get
committed to a repository; content does not move between matter folders, because the wall between two clients' matters
is the point; and the site — not the folder — is authoritative.

The supervision rule from earlier in this class does not relax because the work moved to a terminal. An agent in a
matter folder is a non-lawyer assistant under ABA Model Rule 5.3, exactly as AIDA is in the connector. It can read the
matter, draft against it, and check your work. It does not sign, file, send, or advance a workflow — those are acts you
take deliberately, through Navigator, and Navigator records that you took them.

Open **one** matter folder, not `~/Projects` itself. Scoping the session to a single matter is the same instinct as not
leaving two clients' files open on the same desk: it keeps the model's context on one matter, and it keeps an accidental
cross-matter reference from being possible in the first place.

### Form a Nevada LLC from the command line

The same CLI forms a real Nevada LLC end to end — no browser — and downloads the **filled official Nevada Secretary of
State packet**:

```bash
navigator site login --host https://your-firm.example
navigator site notation create nv__llc_formation --client-email libra@example.com
navigator site intake answer <notation-id>
navigator site notation status <notation-id>
navigator site notation approve <notation-id>
navigator site notation document <notation-id> --out llc.pdf
```

---

You open a questionnaire-driven matter, answer the formation questions at the terminal, and download the same artifact a
browser walk produces — the one you review before the lawyer-gated filing. `notation create` starts the
`nv__llc_formation` Notation and prints its notation id. `intake answer` then walks the formation questionnaire one
question at a time — the entity name, the registered agent, whether the company is member-managed or manager-managed,
and the managing members entered row by row (a blank name ends the list). Answer it interactively, or script it with
repeated `--answer` and `--person` flags. `notation status` reports the workflow state and whether the packet has
already been rendered, then `notation approve` parks the filled packet, and `notation document` writes the PDF to
`--out`. AIDA fills the state's official form from the answers — it never invents one — and the matter ends at the same
lawyer-gated `filing__nv_sos` step a browser walk reaches: **you file with the Secretary of State; Neon Law Navigator
never files for you.**

This whole command-line round-trip is covered by an automated test that drives the real `navigator` binary and checks
the downloaded bytes are the official packet carrying the founder's answers. The pipeline behind the fill — vendoring
the canonical form, mapping answers to its fields, and the lawyer-gated filing that ends it — is laid out step by step
in [Government forms: vendor, map, fill, file](/docs/gov-forms).

### Walk a questionnaire like a text adventure

`intake answer` is a guided loop: it shows one question, you answer it, it shows the next — a text adventure through the
matter's questionnaire.

```bash
navigator site intake answer <notation-id>
```

---

Most questions you simply type — a name, a date (`YYYY-MM-DD`), a numbered choice. But some ask for a **record** or a
**reference**: an answer that is an *existing row*, not a spelling. For those the walk prints a numbered pick-list — a
`#`, the row's name, and its id — and you choose one. The client on the matter is picked from the Project's own people;
a country or jurisdiction is picked from Navigator's seeded reference data. Choosing a row stores that row's id in the
answer, so `{{country__of_birth}}` renders the one canonical *Mexico* every time, never a near-miss spelling of it.

To script the walk instead of typing it, pass a `--select` flag for each pick-list — its value is the number the walk
printed, or the row's id — and an `--answer` flag for each typed question, in the questionnaire's order:

```bash
navigator site intake answer <notation-id> \
  --select person__client=2 \
  --select country__of_birth=114 \
  --answer 1990-04-12
```

Already have the answers on record — a recorded intake call, an email thread, a prior questionnaire? Hand the walk a
transcript and it pre-fills what it can:

```bash
navigator site intake answer <notation-id> --transcript <client-call.txt>
```

Navigator runs the transcript through its coverage engine, proposes an answer for every question the transcript covers,
and walks you through those as **Enter-to-accept defaults**, each labeled *proposed from transcript*. Nothing is
silently accepted: you confirm or correct every proposal, and the questions the transcript never touched still prompt as
normal. The attorney is still the actor — the transcript is only a faster first draft.

## Vibe Code the Navigator

### Your installation publishes its own schema

You do not need to read Rust to build against your matters. Your installation documents itself:

- `https://<your-host>/app/api/openapi.json` — the OpenAPI 3.1 document for every `/app/api/*` endpoint.
- `https://<your-host>/app/api` — the same document rendered as Swagger UI.

Sign in first, then open the second one. Then paste the first one into a coding agent and describe the screen you want.

---

Both URLs are private, like everything else under `/app/api`. The schema is a firm-internal reference — it describes
every lawyer-only command — so it admits the tiers that operate Navigator (`owner`, `admin`, `lawyer`, `clerk`) and
`client` and anonymous alike. Signing in is therefore the first step of building against it, and the credential you use
to read the map is the same one your page will use to call the endpoints on it.

That distinction is what makes vibe coding practical here. The expensive part of building against someone else's system
is normally discovering what exists — which endpoints, which fields, which shapes. Navigator hands you that in one file
a model can read in a single prompt. What is left is the part you are actually good at: deciding what the screen should
do.

### Two doors — `/api` for a page, `/mcp` for an agent

Same database, same authorization, two shapes:

| Door | Protocol | Call it from |
| --- | --- | --- |
| `/app/api/*` | REST over JSON, described by `/app/api/openapi.json` | A page, a script, anything speaking HTTP |
| `/mcp` | JSON-RPC 2.0 over MCP Streamable HTTP | An LLM client — Claude, Gemini, LibreChat, your own agent |

`/mcp` advertises fourteen tools, all namespaced `aida_`: `aida_list_projects`, `aida_create_notation`,
`aida_validate_notation`, `aida_answer_notation`, `aida_show_person`, and the rest.

---

Those are the same tools AIDA used earlier in this class. Navigator keeps one tool catalog and exposes it through both
MCP and A2A rather than maintaining a second one for programmatic callers, so the tool you watched work in Gemini is the
tool your own application gets. Nothing here is a special developer API that drifts from the product.

Pick the door by what you are building. A dashboard, a client-facing status page, a report that has to look a particular
way — that is a page, and it calls `/api`. Something conversational, or something you want a model to drive, is an MCP
client. Many attorneys end up with both in one app: `/api` for what renders, `/mcp` for what reasons.

### A first application is about fifteen lines

Point a fetch at your own host and read the tool catalog back:

```js
const res = await fetch("https://<your-host>/mcp", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    Authorization: `Bearer ${token}`,
  },
  body: JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "tools/list",
  }),
});

const { result } = await res.json();
console.log(result.tools.map((t) => t.name));
```

Swap `tools/list` for `tools/call` with `{"name": "aida_list_projects", "arguments": {}}` and you have your matters.

---

The `token` is the interesting part, and it is deliberately not something your application mints. It is a credential
your installation already issued to a person your installation already knows — a Google OAuth access token on a firm
deployment, a signed bearer on a development one. Navigator validates it, resolves it to a person record, and runs the
same policy decision that guards the lawyer workbench. Your JavaScript is a caller, not a gate.

Say that out loud in the room, because it is the thing that makes vibe coding safe here rather than reckless. An
application built in an afternoon by someone who has never read the authorization model still cannot show a client a
document the portal would not, or reach a matter the signed-in lawyer has no participation on. The answer comes from the
policy layer either way. A bug in your app is a broken screen; it is not a disclosure.

### Vibe the screen — do not vibe the rules

Prototype freely on one side of the line, and call Navigator on the other:

- **Vibe this.** Layout, interaction, the empty state, the error state, what a report looks like when you print it.
- **Call Navigator for this.** Who may see it, what a valid notation is, what a signature means, what gets recorded.

Two rules travel with every prototype: **no client data, ever** — invented or firm-owned names only, and reserved
example domains for any address — and **your prototype is a specification, not a shipment**.

---

The repository states the reasoning plainly: vibing is very good at showing what a screen should feel like, and very bad
at satisfying this codebase's authorization, audit, and durability rules. Splitting the work at that seam lets each side
do what it is good at, and it is why a prototype is judged on whether the states are drawn — empty, loading, error,
success — rather than on whether the code is clean.

The no-client-data rule is not workshop etiquette. Every pull request against the product is scanned for exactly this,
and a mockup carrying real client information is closed and its attachments deleted. Invent your Virgo. It costs nothing
and it is the difference between a prototype you can show anyone and one you cannot show at all.

When a screen you vibed should become part of Navigator itself, file it as a **design mockup** issue — a GIF of the
interaction plus the source that produced it — and an engineer translates it into the real application. What you attach
is read, never merged, never served, and never a dependency. The shipped screen is Rust and it will look like your
design without containing your files.

### Ship it as a Project's client portal

A React application can also live *beside* a matter, at a route Navigator owns:

```text
/app/projects/<project-code>/portal/
```

One Project, one portal. The path is the Project code plus one literal segment, so the mount is derivable from the
repository name alone — there is no application name for anyone to choose, register, or guess.

---

Participation is the gate. Navigator resolves the code to a Project, authorizes the caller through that Project's
participation ledger, and answers a scope miss with **404** rather than 403 — a 403 would confirm to somebody not on the
matter that a Project with this code exists. A code naming no Project gets the identical response, so the status
discloses nothing about which refusal it was.

What changed is that there is nothing to add: a Project either has a portal repository or it does not. What did not
change is that you cannot guess your way into a route.

Two paths, and it is worth being precise about which is which. Standing up an installation and building on it is
**self-serve**: `navigator ops gcp setup` puts the stack on your own Google Cloud project, and from there your `/api`,
your `/mcp`, and whatever you build against them are yours — no conversation with anyone required. Mounting an
application *inside* a matter's route on a deployment the Firm operates is the scoped one, because that route writes to
a Shared Drive folder under a client's matter.

The firm runs applications on that second seam today — practice-specific surfaces, each built for one matter, running on
Navigator at a Project-scoped route with the matter record still in Navigator and the approved output still published to
Drive. That is the worked proof the seam is real.

### The line your application does not cross

Everything you build is a non-lawyer assistant under ABA Model Rule 5.3 — the same standing AIDA has, and the same
standing the agent in your matter folder has. It may read, draft, summarize, and check your work. It does not:

- sign, file, or send;
- advance a workflow step;
- decide who may see a document.

Those are acts an attorney takes deliberately, through Navigator, which records that the attorney took them.

---

Supervision does not relax because the interface got friendlier. This is the same rule from earlier in the class, stated
once more at the point where it is easiest to forget: an application you wrote yourself feels like an extension of your
own judgment, and it is not one. Navigator draws the boundary in code — the write doors that matter are gated and
audited — but the professional obligation is yours regardless of what the software would have allowed.

The other half is provenance. Your application is a working copy the way `navigator site sync` produces a working copy:
useful, current, and not the record. The matter's workflow step, its notations, its filed documents, and its audit trail
live on your installation. Build anything you like on top of that. Do not build something that quietly becomes the truth
instead.

## Prepare the Room Before Class

### Seat every attendee before the first login

Everything above is what an attendee does in class. This section is for the person running it — the environment operator
who seats everyone before anyone signs in. The attendee arc is *arrive as a client → get promoted → work an existing
matter → then open your own*, and it only holds if the room is prepared first. Five steps, once per class:

1. **Start a development environment.** [Deploy the Neon Law Navigator](/workshops/deploy-the-navigator) stands up a
   disposable `dev` instance — local KIND for a dry run, or a cloud staging lane for the room itself. It is throwaway by
   design, so deleting and recreating it between classes is cheap.
2. **Confirm the stock dry run before seating a cohort.** A `dev` environment applies the disposable development
   portfolio automatically on boot — there is no seed command to run. Sign in as `lawyer@neonlaw.com` / `password`, then
   open `/app/projects` and confirm *Henderson Bungalow Purchase*. Sign in separately as `client@neonlaw.com` /
   `password` and confirm the same matter in `/app/projects` through the client lens. The client account's detail files
   are empty until an exercise creates them. If both lists behave that way, the local room is stocked.
3. **Seat each attendee — a Rauthy identity and a Navigator person on the same email.** The two stock accounts are for a
   local dry run only. A class cohort still needs its own identities and matching People: sign-in is operator-mediated:
   an authenticated email with no pre-seeded person is refused, so pre-provisioning is the only door in. For each
   attendee, create a Rauthy user and a Navigator person (`/admin/people/new`) that share the **same email address** —
   email is the join key. Seed them as `client`; the promotion comes next. Attendee emails are real contact data, so
   they live in the running environment only and never in the repository.
4. **Promote and disclose membership through the people surfaces.** Promote each attendee to `lawyer` from the people
   console, then add them to the Henderson matter through the project's people surface. The participation ledger is the
   access grant: record a firm-side participation for every lawyer or Clerk before they open the matter; the accountable
   lawyer is the one whose participation row carries the `is_lawyer_dri` marker, so naming the lawyer DRI and recording
   their participation are one act. Do not treat role promotion or a disclosure/conflict record as a substitute for
   Project participation. Assignment is deliberate: a lawyer user with no project participation sees an empty workbench
   — only an admin sees every matter unassigned. Assigning the attendee to the seeded matter is what turns "work an
   existing matter" from a slogan into a populated screen.
5. **Verify before the doors open.** Have each lawyer attendee sign in once and confirm the lawyer workbench lists the
   Henderson matter. Verify the Virgo client lens separately: it lists the matter, while empty client-file sections are
   expected until an exercise creates approved client-facing artifacts. A green login and the scoped lawyer workbench
   are the signal that the room is ready.

---

The load-bearing step is the membership disclosure in step four. Promotion to `lawyer` alone leaves an empty workbench,
and a lawyer-DRI marker alone is not access, because the lawyer lens is scoped to the participation ledger — only an
admin sees every matter unassigned. It is the seeded Henderson matter *plus* the attendee's firm-side participation on
it that turns "work an existing matter" into a populated screen. The rest is data-minimization: attendee emails are real
contact data, so they live in the running `dev` environment only and never in the repository, which is scanned for
exactly that. A `dev` environment's identity store is disposable, so the roster is re-entered by hand each time you
recreate the environment — cheap for a class, and what keeps attendee contact data out of the tree entirely.

## Wrap Up

### Share what you built

When your three-minute demo is finished, send the markdown of your template — and the one kaizen improvement you found —
to [support@neonlaw.org](mailto:support@neonlaw.org?subject=Workshop+demo).

---

Every template a lawyer contributes raises the floor of competence for the next lawyer who joins. Sharing what you built
is the second of five ways to give back — the [Contributing to Neon Law
Navigator](/workshops/contribute-to-the-navigator) workshop walks all five, from a GitHub issue to a show-and-tell.
