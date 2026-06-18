# views

All HTML for Navigator lives here. Maud components plus full-page templates for the public site (home, about, services,
foundation, blog, workshops) and the admin UI. The crate has no notion of routing, state, or persistence — `web` hands
it data and it returns `Markup`.

## Getting started

```bash
cargo test -p views
```

Most tests render pages with fixture data and assert on substrings that should appear in the output (titles, CTA copy,
auth-aware header swaps). It's fast — pure CPU, no I/O.

The library exposes one module per page (`pages::home`, `pages::admin::retainers`, …) plus a shared layout in
`layout.rs` and brand colors in `brand.rs`. Pico CSS is the visual baseline; the crate ships zero static assets — those
live in `web/public/`.

## What's next

When `web` adds a route, add a matching `pub fn render(...) -> Markup` in `views/src/pages/`. Keep handler-side logic
(form validation, DB lookups) out of views — they should only translate data into markup so the test suite can render
any page from a literal struct.
