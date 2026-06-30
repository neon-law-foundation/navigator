# Frontmatter: the cover sheet on every file

This page is for the attorney who is about to write or edit a file in Neon Law Navigator — a notation template, an event
page, a blog post, or board minutes — and wants to know what the little block at the top is for. You do not need to be a
programmer to read it. You need to know which label goes on which document, and what each line means.

## You cannot quietly ship a broken document

Start here, because it is the part that protects you. Every file is checked as you type, in your editor, against the
same rules the project enforces everywhere else. If you leave out something a document needs — a title, the attorney
review step, the second half of a pair — the editor underlines it in **red** before the file ever leaves your screen.
You are caught at your desk, not in production and not in front of a client. The rest of this page is just *what* the
checker is looking for.

## What frontmatter is

Most files in Neon Law Navigator are plain text, and many of them begin with a small block fenced top and bottom by a
line of three dashes (`---`). The block holds a few `key: value` lines, like this:

```yaml
title: Retainer Agreement
code: onboarding__retainer
```

That block is the **frontmatter** (the real file has a `---` line above and below it). Think of it as the caption on a
pleading: a short, structured label that tells the system *what kind of document this is* and the handful of facts it
needs to file it correctly. Everything below the block is the document itself — the prose you write and, in the end,
sign.

The format is called YAML, but it is nothing more than `key: value`, one per line. There is no programming. Spell the
key correctly on the left, put a valid value on the right, and keep the indentation the examples show. When something is
wrong, the editor underlines it — the same way a word processor underlines a misspelling.

## The kinds of file, and what each one declares

Neon Law Navigator works out a file's kind by **reading it**, not by asking you. A file whose frontmatter declares a
`questionnaire:` or `workflow:` block is a notation template — wherever it lives; the `templates/` folder is a
convention, not the signal. A file with a `starts_at:` time is an event; a file under the blog or board-minutes folders
is that kind of page; everything else is ordinary prose and is held only to general writing rules. Each kind and the
keys it must carry:

- **Notation template** — declaring `questionnaire:` **or** `workflow:` is what makes a file a template (either block on
  its own draws the template rules). A complete one then carries **both** machines plus `title`, `code`,
  `respondent_type`, `jurisdiction`, and `confidential`, and the missing ones are flagged. Lives under `templates/` by
  convention; a `templates/` file with neither block yet is just prose until it declares one.
- **Event page** — lives under `web/content/events/`. Needs `title`, `description`, `starts_at`, `timezone`, and a
  `location_address` or `meeting_url`.
- **Blog post** — lives under `web/content/blog/`. Needs `title` and `description`, in a file named `YYYYMMDD_slug.md`.
- **Board minutes** — live under `web/content/foundation/minutes/`. Need `title` and `description`, in a file named
  `YYYY-qN.md`.
- **Everything else** — ordinary prose (READMEs, docs). No frontmatter is required.

## Notation templates — the legal blueprints

A notation template is the document a client eventually signs, plus the questions that fill it in and the path it walks
to get there. Here is the real frontmatter from the shared retainer, `templates/neon_law/shared/retainer.md` (shown
without its surrounding `---` fences):

```yaml
title: Retainer Agreement
respondent_type: person_and_entity
code: onboarding__retainer
jurisdiction: NV
confidential: true
questionnaire:
  BEGIN:               { _: client_name }
  client_name:         { _: client_email }
  client_email:        { _: project_name }
  project_name:        { _: product_description }
  product_description: { _: END }
  END: {}
workflow:
  BEGIN:                       { intake_submitted: intake_persisted__client }
  intake_persisted__client:    { retainer_rendered: staff_review }
  staff_review:                { approved: document_open__retainer_pdf, rejected: END }
  document_open__retainer_pdf: { pdf_persisted: sent_for_signature__pending }
  sent_for_signature__pending: { signature_received: END, signature_declined: END }
  END: {}
```

Each key, in plain English:

- **`title`** — the human name of the document, e.g. `Retainer Agreement`. It cannot be blank.
- **`code`** — the document's permanent file number, in `snake_case` (e.g. `onboarding__retainer`). It must be unique
  across the whole project, and you do not change it once clients have signed under it. The reason is the record: the
  `code` is how a signed document is traced back to the blueprint it came from, so changing it later would cut the audit
  trail your malpractice carrier may one day need to read.
- **`respondent_type`** — who signs: `person`, `entity`, or `person_and_entity`. Nothing else is accepted.
- **`jurisdiction`** — the state whose law governs: `NV`, `CA`, or `US`.
- **`confidential`** — `true` or `false`. There is no default; you state it on purpose, every time, because the system
  will not guess how to treat a client's document for you.
- **`questionnaire`** — the questions the client answers, written as a simple step-by-step ladder from `BEGIN` to `END`.
- **`workflow`** — the path the document walks from intake to signature. It **must** include a `staff_review` step. That
  is not a formality: a licensed attorney reviews the draft before anything is sent — the supervision you owe any
  non-lawyer assistant (ABA Model Rule 5.3). The document is never sent, filed, or signed on its own.

### One rule worth saying twice: `questionnaire` and `workflow` travel together

A notation template has **both** `questionnaire:` and `workflow:`, or neither. If you write one and forget the other,
the checker stops you. A blueprint with questions but no path — or a path but no questions — is half a document, and a
half-built document should never reach a client. This is a guardrail, not a nicety.

The body below the frontmatter is the legal prose, in English, carrying `{{placeholder}}` slots that the questionnaire
answers fill in (`{{client_name}}`, `{{project_name}}`, and so on). Authoring that body, and the full list of structural
checks, is covered in <notation-authoring.md>.

### How the finished document looks: `output`

A notation template may carry an optional **`output`** key. It is the one place a template declares its **render
profile** — what comes out and how it is dressed:

- **omit it** (the default) and the document renders as a plain page — our standard serif, one-inch margins, no
  letterhead. The body's `{{placeholders}}` fill from the questionnaire answers.
- **`output: letter`** renders the same body on Neon Law letterhead: our logo, the firm name and contact line, a rule
  across the top. This is the dressing we use for the documents that go out under the firm's name, such as engagement
  letters and demand letters.
- **`output: form`** is a different mode entirely: instead of typesetting prose, it prints the questionnaire answers
  onto an official government form (an AcroForm fill). A `form` template carries no legal prose — its body is the field
  map — so it always rides with the two form keys below (`form:` and `origin_url:`), and the checker (N109) requires
  them. Conversely a typeset profile (`letter`, or no `output:` at all) must **not** carry a `form:` key.

`letter` and `form` are the values the checker accepts today (N109); leaving the key off gives you the plain page. As we
add court-specific layouts (pleading paper), each becomes one more named value here — so `output` stays the one place a
template says what it should look like.

### Government form templates carry two extra keys

A template backed by an official government form (under `templates/forms/`) declares `output: form` and adds `form:`
(the form's identity) and `origin_url:` (the official `.gov` page the blank form came from), as in
`templates/forms/united_states/nevada/state/nv__llc_formation.md`:

```yaml
title: Neon Law Nest — Nevada Entity Formation
respondent_type: person_and_entity
code: nv__llc_formation
jurisdiction: NV
origin_url: https://www.nvsos.gov/businesses/commercial-recordings/forms-fees/all-business-forms
confidential: false
output: form
form: nv__llc_formation
```

The three travel together: N109 requires `form:` and `origin_url:` whenever `output: form` is declared, and rejects a
`form:` key on any other profile. So `form:` present and `output: form` always imply each other.

## Event pages

An event page (a public show-and-tell) is dated, so it carries a start time on top of a title and description. From
`web/content/events/`:

```yaml
title: "Salt Lake City Nebula Show and Tell"
description: >
  A Salt Lake City session for practical legal AI workflows, demos, peer review, and responsible adoption habits.
draft: true
starts_at: "2026-07-20T11:00:00"
ends_at: "2026-07-20T15:00:00"
timezone: America/Denver
location_address: Salt Lake City, Utah
```

- **`title`** and **`description`** — the name and the one-line summary (the summary becomes the page's search and
  social preview, so it cannot be blank).
- **`starts_at`** and **`timezone`** — when it begins, and in which timezone. Both are required.
- **`location_address`** or **`meeting_url`** — where to show up, in person or online (a hybrid event may give both).

The `description: >` you see is just a way to wrap one long sentence across several lines; it still reads as a single
sentence.

## Blog posts and board minutes

These two are the simplest: a `title` and a `description`, and a filename that follows a fixed shape.

A blog post (`web/content/blog/`) takes its publish date from the filename, so the name **must** be `YYYYMMDD_slug.md`
(e.g. `20260625_going_all_in_on_rust.md`). A name whose date does not parse is silently dropped — the post never shows
up and never errors — so the checker flags a bad name for you.

```yaml
title: Going All-In on Rust
description: Why Neon Law Foundation chose one language for fast, safe, local-first access-to-justice software.
```

Board minutes (`web/content/foundation/minutes/`) are one file per quarter, named `YYYY-qN.md` (e.g. `2026-q1.md`):

```yaml
title: "Board Meeting Minutes — Q2 2023"
description: "Minutes of the Neon Law Foundation board of directors for the Q2 2023 regular meeting."
```

## Every frontmatter key at a glance

The narrative above covers the keys you reach for daily. This table is the complete set the system knows, grouped by
document kind, so nothing is hidden:

### Notation template

| Key | Required | Values | Checked by |
| --- | --- | --- | --- |
| `title` | yes | any non-empty text | N101 |
| `code` | yes | unique `snake_case` | N108 |
| `respondent_type` | yes | `person`, `entity`, `person_and_entity` | N102 |
| `jurisdiction` | yes | `NV`, `CA`, `US` | N110 |
| `confidential` | yes | `true` or `false` | N105 |
| `questionnaire` | yes (paired) | a `BEGIN` → `END` ladder | N104 |
| `workflow` | yes (paired) | a `BEGIN` → `END` path that includes `staff_review` | N104, N106 |
| `prompts` | no | wording for custom questions | N104 |
| `output` | no | `letter` or `form` (omit for a plain page) | N109 |
| `form` | with `output: form` | the bundled form's code | N109 |
| `origin_url` | forms only | the `.gov` page the blank form came from | N109, N110 |

### Event page

| Key | Required | Values | Checked by |
| --- | --- | --- | --- |
| `title` | yes | any non-empty text | C001 |
| `description` | yes | any non-empty text | C002 |
| `starts_at` | yes | an ISO-8601 time | E001 |
| `timezone` | yes | an IANA zone, e.g. `America/Denver` | E001 |
| `location_address` or `meeting_url` | one of the two | a place or a link | E003 |
| `ends_at` | no | an ISO-8601 time | web build |
| `draft` | no | `true` or `false` | web build |
| `location_name` | no | a venue name | web build |
| `image_url`, `image_alt` | no | a preview image and its alt text | web build |
| `video_url`, `recap_url` | no | links to a recording or a recap | web build |
| `public_slug` | no | a custom URL slug | web build |

### Blog post and board minutes

| Key | Required | Values | Checked by |
| --- | --- | --- | --- |
| `title` | yes | any non-empty text | C001 |
| `description` | yes | any non-empty text | C002 |

Two footnotes. `form` rides along on government-form templates and is bound to `output: form` — N109 requires the two
together and rejects a `form:` key on any other profile, so a stray or orphaned `form:` is now a loud error rather than
a silent one. The event keys marked "web build" are read when the page is rendered rather than by the command-line
checker, so they will not underline in your editor.

## The squiggly underline: red versus yellow

Open any of these files in a supported editor and the checker runs as you type:

- a **red** underline is a blocking error — a missing `title`, an unknown `respondent_type`, a workflow with no
  `staff_review`, a half-declared template, a blog filename that will not publish. The file is not done until the red is
  gone.
- a **yellow** underline is a non-blocking heads-up — most often a workflow step that is allowed but whose automation is
  not built yet. It is information, not a blocker.

Hover over an underline and it tells you the rule and what to fix. Nothing you type leaves your machine: the checker
reads only the buffer in front of you and sends nothing anywhere, which is the same confidentiality the `confidential`
flag is there to protect. Editor setup is in <lsp/README.md>.

## Checking it yourself from the command line

The editor checks continuously, but you can run the same checker by hand over a file or folder:

```bash
cargo run -p cli --quiet -- validate --no-default-excludes <path>
```

It classifies each file automatically — a template is held to the template rules, a blog post to the blog rules, prose
to the writing rules — and prints any problem with its file, line, and rule code. (There used to be a `--markdown-only`
switch; it is no longer needed and is ignored, because the checker now works out each file's kind on its own.)

## Where to go next

- <notation-authoring.md> — how to author the body of a notation template and the full validation contract.
- The Neon Law Navigator workshop — a hands-on walk that builds one real notation end to end.
