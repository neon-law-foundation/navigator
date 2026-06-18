---
name: vendor-gov-forms
description: >
  Acquire and vendor government forms (and any other official document we file, fill, or cite) from their canonical
  source — the issuing authority's own domain — with provenance a reader can verify from commit history. Trigger when
  downloading any .gov PDF (Nevada SoS, IRS, county recorder, courts), when adding a form-backed template, when a fetch
  of a government site returns 403 (bot walls are normal; mirrors are not the fallback), or before reaching for the
  Wayback Machine, a forms aggregator, or any rehosted copy. Also trigger before authoring a field map, overlay map, or
  form template — the canonical example must be on disk first, no guessing. Also trigger when refreshing a vendored form
  — the authorities revise forms silently, and the refresh commit is our timestamp ledger.
---

# Vendoring government forms from canonical sources

The artifact we fill and file with an authority must be that authority's own current bytes, and we must be able to prove
where every byte came from and when we took it. Both halves are load-bearing: a clerk can reject a stale or altered
form, and a provenance question years later is answered by `git log`, not by memory.

## The rule: canonical source only

- **Source every form from the issuing authority's own domain** — `nvsos.gov`, `irs.gov`, the court's or county's
  official site. The same rule applies to statutes, fee schedules, and instructions we rely on.
- **Never** substitute the Wayback Machine, a forms aggregator, a search-result rehost, or a "helpful" mirror — not
  even to unblock yourself when the authority's site is bot-walled. An archive proves what a page *was*, not what the
  authority publishes *today*, and a mirror proves nothing at all.
- If the canonical URL is unreachable, the answer is a different *acquisition path* (see below), never a different
  *source*.

## Canonical example on disk — no guessing

A vendored form's exact bytes are **committed to the repo** beside the ledger, at
`templates/forms/<authority>/<form_code>-<revision>.pdf`. Nothing — no field map, no overlay coordinate map, no template
body — is authored until those bytes are on disk, and everything is derived from a dump of *those bytes*:

- Field names come from walking the PDF's AcroForm with `lopdf` (the `pdf` crate's own dependency), never from the
  form's web page, its instructions, a different revision, memory, or inference. Real government forms make guessing
  fatal: the Nevada SoS formation packets carry OmniForm field names like `undefined`, `City_5`, and `1 Name of Entity
  If foreign name in home jurisdiction` — no one guesses those.
- Overlay coordinates for flat forms are measured against the on-disk bytes, never estimated from a screenshot.
- The guard test recomputes each ledger `sha256` from the on-disk file, so "the example we derived from" and "the
  example in the repo" cannot drift apart.

## Provenance: `FORMS.toml` + commit history

Every vendored form is pinned in a `FORMS.toml` ledger, modeled byte-for-byte on
[`web/public/VENDOR.toml`](../../../web/public/VENDOR.toml). The ledger lives beside the templates that consume the
forms; if the form-template gallery moves, the ledger moves with it. One entry per form revision:

```toml
[[form]]
authority = "Nevada Secretary of State"
name = "Articles of Organization (NRS 86)"
form_code = "nv_sos__llc_articles"          # stable id; templates reference this
revision = "2023-12"                        # the revision date printed on the form itself
source_url = "https://www.nvsos.gov/..."    # the exact canonical URL the bytes came from
retrieved = "2026-06-12"                    # the day we pulled the bytes
sha256 = "<sha256 of the PDF bytes>"
fill = "acroform"                           # acroform | overlay | none (see classification below)
object_path = "forms/nv_sos/nv_sos__llc_articles-2023-12.pdf"
```

- A guard test recomputes each `sha256` from the stored bytes and fails on drift — same pattern as
  `web/tests/vendor_assets.rs`, so the ledger cannot lie.
- **One commit per acquisition or refresh**, touching the ledger entry and nothing unrelated. The commit date is the
  verifiable timestamp; `git log --follow FORMS.toml` is the audit trail. Never batch a form refresh into a feature
  commit.

## Acquisition: bot walls are the normal case

Government sites commonly sit behind bot protection. `nvsos.gov` runs Imperva Incapsula: every non-browser client (curl
with any header set, WebFetch, plain reqwest) gets a 403 or a JavaScript-challenge iframe, never the PDF. Do not burn
time on header tricks, and do not fall back to a mirror. The acquisition paths, in order:

1. **User-run browser download.** Propose the exact canonical URLs and have the user download them in their browser
   (or `!`-run a command), dropping the files in `/tmp/` for verification. This is the default path and is consistent
   with this workspace's rule that machine-bound actions run on the user's machine.
2. **Real-browser automation** via `fantoccini` + chromedriver (the same stack as `web/tests/browser_e2e.rs`), which
   passes JavaScript challenges because it *is* a browser. Worth building only when refreshing at scale (thousands of
   forms), and it stays Rust-only.

Verify immediately after acquisition, before anything consumes the file:

```bash
file <form>.pdf                  # must say "PDF document", not HTML
sha256sum <form>.pdf             # goes into FORMS.toml
```

## Classify the fill strategy

Record how the form can be filled in the ledger's `fill` field — it decides which rendering path a template uses:

- `acroform` — the PDF has an AcroForm field tree; `pdf::fill_acroform` fills it by field name. Check with
  `grep -c /AcroForm <form>.pdf` (or `pdf::acroform::read_field_value` in a test).
- `overlay` — flat scan, no fields; answers are stamped as positioned text boxes over the page (coordinate map per
  form). Use only when the authority publishes no fillable variant.
- `none` — reference documents we cite but never fill (instructions, fee schedules).
- **XFA forms are rejected loudly** (`PdfError::XfaUnsupported`); find the authority's non-XFA alternative rather
  than working around it.

## Storage: canonical bytes in the repo, a serving copy in the bucket

- The canonical copy is the committed file under `templates/forms/` (see "no guessing" above) — git history is its
  audit trail and the guard test pins it to the ledger.
- The same bytes are uploaded to the assets bucket (`NAVIGATOR_ASSETS_BUCKET`, by convention `<project>-assets`) at
  the ledger's `object_path`, and the website serves blank-form downloads to **logged-in** users from there. Bucket
  names flow through `.env`; never hard-code a project id here or in any doc this skill produces.
- A **filled** form is a client document and never goes to the assets bucket — rendered output persists through
  `cloud::StorageService` into the private documents bucket, exactly like every other notation PDF.

## Refresh discipline

Authorities revise forms without notice. When a template's form is touched (new matter shipping, periodic sweep, or a
clerk rejection):

1. Re-acquire from the same canonical `source_url`.
2. Compare `sha256` and the printed revision date against the ledger.
3. If changed: upload the new bytes to a **new** `object_path` (revisions are append-only, old bytes stay), update the
   ledger entry, and re-check the template's field map against the new field names — a silent field rename is the
   failure mode this discipline exists to catch.
4. Commit the refresh on its own, so the history reads as "the authority changed the form on this date, we caught it
   on this date."
