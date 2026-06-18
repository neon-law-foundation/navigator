# pdf

PDF rendering for Navigator's legal documents. Backed by the [Typst](https://typst.app) embedded compiler via the
`typst-as-lib` wrapper: callers feed Typst markup to `render` and return the PDF bytes; persistence is the caller's job
through the `cloud::StorageService` seam, so this crate stays I/O-free. Consumed by `web`, `workflows`, and `features`.

## What it provides

- `render(&str) -> Vec<u8>` — compile Typst markup to PDF bytes.
- `render_with_redactions(...)` — the same, applying redaction marks.
- `RedactionStyle` — `Block` (solid rectangle), `Bar` (the classic centred "with prejudice" mark), or `Strike`
  (review-mode strikethrough on still-legible text).
- `acroform::{blank_acroform, fill_acroform, read_field_value}` — author and fill AcroForm fields for form PDFs.

## Fonts

Every PDF this crate renders is set in **Noto Serif**, the firm's typeface — a sturdy, screen-and-print legible serif
whose broad Unicode coverage (Latin + all European accents, Cyrillic, Greek, Vietnamese) keeps client names and
addresses rendering correctly worldwide. Two Google Fonts variable masters (upright + italic, `wght` axis) are embedded
into the binary via `include_bytes!` from `pdf/assets/fonts/NotoSerif/`; Typst instantiates Regular and Bold off the
weight axis. `render` prepends a `#set text(font: "Noto Serif")` rule so the family is the document default; a caller
can still override it. The same typeface is served self-hosted by `web` (`web/public/fonts/noto-serif/`).

Noto Serif ships under the SIL Open Font License 1.1; the full text is at `pdf/assets/fonts/NotoSerif/OFL.txt`.

## Getting started

```bash
# Render round-trips + redaction styles + AcroForm fill. No fonts to install — they're embedded.
cargo test -p pdf
```
